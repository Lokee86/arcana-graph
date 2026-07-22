package main

import (
	"fmt"
	"go/ast"
	"go/token"
	"go/types"
	"path/filepath"
	"sort"
	"strings"

	"golang.org/x/tools/go/packages"
)

type semanticCall struct {
	target    NodeKey
	resolved  bool
	reason    UnresolvedReason
	namespace string
	name      string
}

type semanticTargets struct {
	byObject map[*types.Func]NodeKey
	byID     map[string][]NodeKey
}

func (s *scanner) loadSemanticCalls() error {
	config := &packages.Config{
		Mode: packages.NeedName | packages.NeedFiles | packages.NeedCompiledGoFiles |
			packages.NeedSyntax | packages.NeedTypes | packages.NeedTypesInfo |
			packages.NeedImports | packages.NeedDeps | packages.NeedModule,
		Dir:   s.root,
		Tests: true,
	}
	roots, err := packages.Load(config, "./...")
	if err != nil {
		return fmt.Errorf("load Go packages: %w", err)
	}
	loaded := flattenPackages(roots)
	for _, pkg := range loaded {
		s.summary.SemanticErrors += len(pkg.Errors)
	}

	targets := semanticTargets{
		byObject: make(map[*types.Func]NodeKey),
		byID:     make(map[string][]NodeKey),
	}
	for _, pkg := range loaded {
		s.collectSemanticTargets(pkg, &targets)
	}
	for _, pkg := range loaded {
		s.collectSemanticCalls(pkg, targets)
	}
	return nil
}

func flattenPackages(roots []*packages.Package) []*packages.Package {
	seen := make(map[string]*packages.Package)
	var visit func(*packages.Package)
	visit = func(pkg *packages.Package) {
		if pkg == nil || seen[pkg.ID] != nil {
			return
		}
		seen[pkg.ID] = pkg
		paths := make([]string, 0, len(pkg.Imports))
		for path := range pkg.Imports {
			paths = append(paths, path)
		}
		sort.Strings(paths)
		for _, path := range paths {
			visit(pkg.Imports[path])
		}
	}
	for _, root := range roots {
		visit(root)
	}
	result := make([]*packages.Package, 0, len(seen))
	for _, pkg := range seen {
		result = append(result, pkg)
	}
	sort.Slice(result, func(i, j int) bool { return result[i].ID < result[j].ID })
	return result
}

func (s *scanner) collectSemanticTargets(pkg *packages.Package, targets *semanticTargets) {
	if pkg.TypesInfo == nil || pkg.Fset == nil {
		return
	}
	for _, file := range pkg.Syntax {
		rel, ok := s.semanticFilePath(pkg.Fset, file)
		if !ok {
			continue
		}
		importPath := s.importPathFor(rel)
		for _, declaration := range file.Decls {
			function, ok := declaration.(*ast.FuncDecl)
			if !ok {
				continue
			}
			object, ok := pkg.TypesInfo.Defs[function.Name].(*types.Func)
			if !ok {
				continue
			}
			key := declarationKey(importPath, rel, function)
			if _, exists := s.nodes[key]; !exists {
				continue
			}
			targets.byObject[object] = key
			id := semanticFunctionID(object)
			targets.byID[id] = appendUniqueKey(targets.byID[id], key)
		}
	}
}

func (s *scanner) collectSemanticCalls(pkg *packages.Package, targets semanticTargets) {
	if pkg.TypesInfo == nil || pkg.Fset == nil {
		return
	}
	for _, file := range pkg.Syntax {
		rel, ok := s.semanticFilePath(pkg.Fset, file)
		if !ok {
			continue
		}
		importPath := s.importPathFor(rel)
		for _, declaration := range file.Decls {
			function, ok := declaration.(*ast.FuncDecl)
			if !ok || function.Body == nil {
				continue
			}
			source := declarationKey(importPath, rel, function)
			if _, exists := s.nodes[source]; !exists {
				continue
			}
			ast.Inspect(function.Body, func(node ast.Node) bool {
				if node != function.Body {
					if _, nested := node.(*ast.FuncLit); nested {
						return false
					}
				}
				call, ok := node.(*ast.CallExpr)
				if !ok {
					return true
				}
				span := spanFromSet(pkg.Fset, call.Pos(), call.End(), rel)
				key := callsiteKey(source, span)
				resolution := s.resolveTypedCall(pkg.TypesInfo, call, targets)
				s.mergeSemanticCall(key, resolution)
				return true
			})
		}
	}
}

func (s *scanner) resolveTypedCall(
	info *types.Info,
	call *ast.CallExpr,
	targets semanticTargets,
) semanticCall {
	object := calledObject(info, call.Fun)
	switch object := object.(type) {
	case *types.Builtin:
		return semanticCall{reason: ReasonBuiltinTarget, namespace: "go:builtins", name: object.Name()}
	case *types.TypeName:
		return semanticCall{reason: ReasonTypeConversion, namespace: objectNamespace(object), name: object.Name()}
	case *types.Func:
		if target, exists := targets.byObject[object]; exists {
			return semanticCall{target: target, resolved: true}
		}
		candidates := targets.byID[semanticFunctionID(object)]
		if len(candidates) == 1 {
			return semanticCall{target: candidates[0], resolved: true}
		}
		namespace := objectNamespace(object)
		if len(candidates) > 1 {
			return semanticCall{reason: ReasonAmbiguousTarget, namespace: namespace, name: object.Name()}
		}
		if !isInternalNamespace(namespace, s.module) {
			return semanticCall{reason: ReasonExternalTarget, namespace: namespace, name: object.Name()}
		}
		return semanticCall{reason: ReasonMissingTarget, namespace: namespace, name: object.Name()}
	case nil:
		reason, namespace, name := classifyNonIdentifier(call.Fun)
		return semanticCall{reason: reason, namespace: namespace, name: name}
	default:
		return semanticCall{reason: ReasonDynamicTarget, name: expressionName(call.Fun)}
	}
}

func calledObject(info *types.Info, expression ast.Expr) types.Object {
	switch expression := expression.(type) {
	case *ast.Ident:
		return info.Uses[expression]
	case *ast.SelectorExpr:
		if selection := info.Selections[expression]; selection != nil {
			return selection.Obj()
		}
		return info.Uses[expression.Sel]
	case *ast.IndexExpr:
		return calledObject(info, expression.X)
	case *ast.IndexListExpr:
		return calledObject(info, expression.X)
	case *ast.ParenExpr:
		return calledObject(info, expression.X)
	default:
		return nil
	}
}

func (s *scanner) semanticFilePath(set *token.FileSet, file *ast.File) (string, bool) {
	filename := set.PositionFor(file.Pos(), false).Filename
	rel, err := s.relative(filename)
	if err != nil || !isGoFile(rel) {
		return "", false
	}
	return rel, true
}

func (s *scanner) importPathFor(rel string) string {
	dir := filepath.ToSlash(filepath.Dir(rel))
	if dir == "" || dir == "." {
		return s.module
	}
	return s.module + "/" + dir
}

func semanticFunctionID(function *types.Func) string {
	if origin := function.Origin(); origin != nil {
		function = origin
	}
	namespace := objectNamespace(function)
	signature, _ := function.Type().(*types.Signature)
	if signature != nil && signature.Recv() != nil {
		return "method:" + namespace + ":" + receiverTypeName(signature.Recv().Type()) + "." + function.Name()
	}
	return "function:" + namespace + ":" + function.Name()
}

func receiverTypeName(value types.Type) string {
	if pointer, ok := value.(*types.Pointer); ok {
		return "*" + receiverTypeName(pointer.Elem())
	}
	if named, ok := value.(*types.Named); ok {
		return named.Obj().Name()
	}
	return types.TypeString(value, func(*types.Package) string { return "" })
}

func objectNamespace(object types.Object) string {
	if object == nil || object.Pkg() == nil {
		return ""
	}
	return object.Pkg().Path()
}

func isInternalNamespace(namespace, module string) bool {
	return namespace == module || strings.HasPrefix(namespace, module+"/") ||
		strings.HasPrefix(namespace, module+"_test")
}

func appendUniqueKey(keys []NodeKey, key NodeKey) []NodeKey {
	for _, existing := range keys {
		if existing == key {
			return keys
		}
	}
	return append(keys, key)
}

func (s *scanner) mergeSemanticCall(key string, incoming semanticCall) {
	existing, exists := s.semanticCalls[key]
	if !exists || (!existing.resolved && incoming.resolved) {
		s.semanticCalls[key] = incoming
		return
	}
	if existing.resolved && incoming.resolved && existing.target != incoming.target {
		s.semanticCalls[key] = semanticCall{reason: ReasonAmbiguousTarget}
	}
}
