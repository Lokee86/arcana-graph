package main

import (
	"bytes"
	"fmt"
	"go/ast"
	"go/printer"
	"go/token"
	"path/filepath"
	"strings"
)

func (s *scanner) parseDeclaration(declaration ast.Decl, pkg packageInfo, path string) {
	switch declaration := declaration.(type) {
	case *ast.GenDecl:
		for _, specification := range declaration.Specs {
			typeSpec, ok := specification.(*ast.TypeSpec)
			if !ok {
				continue
			}
			key := hashIdentity("type:" + pkg.importKey + ":" + typeSpec.Name.Name)
			node := NodeFact{Key: key, Kind: KindType, Path: path, Name: typeSpec.Name.Name, Span: s.span(typeSpec.Pos(), typeSpec.End(), path)}
			s.addNode(node)
			s.addEdge(pkg.key, key, RelDefines, node.Span)
		}
	case *ast.FuncDecl:
		kind, identity := declarationIdentity(pkg.importKey, path, declaration)
		key := hashIdentity(identity)
		node := NodeFact{Key: key, Kind: kind, Path: path, Name: declaration.Name.Name, Span: s.span(declaration.Pos(), declaration.End(), path)}
		s.addNode(node)
		s.addEdge(pkg.key, key, RelDefines, node.Span)

		scope := pkg.importKey + "\x00" + pkg.name
		if declaration.Recv == nil {
			s.targets[scope] = appendTarget(s.targets[scope], declaration.Name.Name, key)
		}
		if declaration.Body != nil {
			s.callables = append(s.callables, callable{
				packageKey: scope,
				namespace:  pkg.importKey,
				source:     key,
				body:       declaration.Body,
				path:       path,
			})
		}
	}
}

func declarationIdentity(importPath, path string, declaration *ast.FuncDecl) (NodeKind, string) {
	name := declaration.Name.Name
	if declaration.Recv != nil {
		receiver := receiverName(declaration.Recv)
		return KindMethod, "method:" + importPath + ":" + receiver + "." + name
	}
	if strings.HasSuffix(path, "_test.go") && strings.HasPrefix(name, "Test") {
		return KindTest, "test:" + importPath + ":" + name
	}
	return KindFunction, "function:" + importPath + ":" + name
}

func declarationKey(importPath, path string, declaration *ast.FuncDecl) NodeKey {
	_, identity := declarationIdentity(importPath, path, declaration)
	return hashIdentity(identity)
}

func appendTarget(targets map[string][]NodeKey, name string, key NodeKey) map[string][]NodeKey {
	if targets == nil {
		targets = make(map[string][]NodeKey)
	}
	targets[name] = append(targets[name], key)
	return targets
}

func receiverName(fields *ast.FieldList) string {
	if fields == nil || len(fields.List) == 0 {
		return ""
	}
	return expressionName(fields.List[0].Type)
}

func expressionName(expression ast.Expr) string {
	switch expression := expression.(type) {
	case *ast.Ident:
		return expression.Name
	case *ast.StarExpr:
		return "*" + expressionName(expression.X)
	case *ast.SelectorExpr:
		return expressionName(expression.X) + "." + expression.Sel.Name
	case *ast.IndexExpr:
		return expressionName(expression.X)
	case *ast.IndexListExpr:
		return expressionName(expression.X)
	case *ast.ParenExpr:
		return expressionName(expression.X)
	default:
		return "anonymous"
	}
}

func (s *scanner) addCallEdges() {
	for _, function := range s.callables {
		targets := s.targets[function.packageKey]
		ast.Inspect(function.body, func(node ast.Node) bool {
			if node != function.body {
				if _, nested := node.(*ast.FuncLit); nested {
					return false
				}
			}
			call, ok := node.(*ast.CallExpr)
			if !ok {
				return true
			}
			s.summary.CallExpressions++
			span := s.span(call.Pos(), call.End(), function.path)
			if resolution, exists := s.semanticCalls[callsiteKey(function.source, span)]; exists {
				if resolution.resolved {
					s.addEdge(function.source, resolution.target, RelCalls, span)
					s.summary.DirectCalls++
				} else {
					s.addUnresolved(function, call, resolution.reason, resolution.namespace, resolution.name)
				}
				return true
			}

			identifier, ok := call.Fun.(*ast.Ident)
			if !ok {
				reason, namespace, name := classifyNonIdentifier(call.Fun)
				s.addUnresolved(function, call, reason, namespace, name)
				return true
			}
			candidates := targets[identifier.Name]
			if len(candidates) == 0 {
				reason := ReasonMissingTarget
				namespace := function.namespace
				if isBuiltin(identifier.Name) {
					reason = ReasonBuiltinTarget
					namespace = "go:builtins"
				}
				s.addUnresolved(function, call, reason, namespace, identifier.Name)
				return true
			}
			if len(candidates) > 1 {
				s.addUnresolved(function, call, ReasonAmbiguousTarget, function.namespace, identifier.Name)
				return true
			}
			target := candidates[0]
			if target == function.source {
				s.addUnresolved(function, call, ReasonSelfTarget, function.namespace, identifier.Name)
				return true
			}
			s.addEdge(function.source, target, RelCalls, span)
			s.summary.DirectCalls++
			return true
		})
	}
}

func (s *scanner) addUnresolved(
	function callable,
	call *ast.CallExpr,
	reason UnresolvedReason,
	namespace string,
	name string,
) {
	s.facts.Unresolved = append(s.facts.Unresolved, UnresolvedReferenceFact{
		Source: function.source, Relation: RelCalls, Expression: expressionText(s.set, call.Fun),
		CandidateNamespace: namespace, CandidateName: name, Reason: reason,
		Span: s.span(call.Pos(), call.End(), function.path),
	})
	s.summary.UnresolvedCalls++
	s.trackUnresolvedReason(reason)
}

func (s *scanner) trackUnresolvedReason(reason UnresolvedReason) {
	switch reason {
	case ReasonBuiltinTarget:
		s.summary.BuiltinCalls++
	case ReasonTypeConversion:
		s.summary.ConversionCalls++
	case ReasonExternalTarget:
		s.summary.ExternalCalls++
	case ReasonDynamicTarget:
		s.summary.DynamicCalls++
	}
}

func classifyNonIdentifier(expression ast.Expr) (UnresolvedReason, string, string) {
	if selector, ok := expression.(*ast.SelectorExpr); ok {
		return ReasonUnsupportedForm, expressionName(selector.X), selector.Sel.Name
	}
	return ReasonDynamicTarget, "", expressionName(expression)
}

func expressionText(set *token.FileSet, expression ast.Expr) string {
	var output bytes.Buffer
	if err := printer.Fprint(&output, set, expression); err != nil {
		return expressionName(expression)
	}
	return output.String()
}

func isBuiltin(name string) bool {
	switch name {
	case "append", "cap", "clear", "close", "complex", "copy", "delete", "imag", "len",
		"make", "max", "min", "new", "panic", "print", "println", "real", "recover":
		return true
	default:
		return false
	}
}

func (s *scanner) span(start, end token.Pos, path string) *SourceSpan {
	return spanFromSet(s.set, start, end, path)
}

func spanFromSet(set *token.FileSet, start, end token.Pos, path string) *SourceSpan {
	if !start.IsValid() || !end.IsValid() {
		return nil
	}
	begin := set.PositionFor(start, false)
	finish := set.PositionFor(end, false)
	return &SourceSpan{
		Path: path, StartLine: uint32(begin.Line), StartColumn: uint32(begin.Column),
		EndLine: uint32(finish.Line), EndColumn: uint32(finish.Column),
	}
}

func callsiteKey(source NodeKey, span *SourceSpan) string {
	if span == nil {
		return fmt.Sprintf("%016x/-", source)
	}
	return fmt.Sprintf("%016x/%s/%d/%d/%d/%d", source, span.Path, span.StartLine,
		span.StartColumn, span.EndLine, span.EndColumn)
}

func isGoFile(path string) bool { return filepath.Ext(path) == ".go" }
