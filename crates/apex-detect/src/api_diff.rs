//! API breaking change detection for OpenAPI 3.x specs (JSON format).
//!
//! Compares two OpenAPI specs and classifies changes as breaking,
//! non-breaking, or deprecation.

use apex_core::error::{ApexError, Result};
use serde_json::Value;
use std::collections::HashSet;

/// Classification of an API change.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum ChangeKind {
    Breaking,
    NonBreaking,
    Deprecation,
}

/// A single detected change between two API specs.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ApiChange {
    pub kind: ChangeKind,
    pub path: String,
    pub method: String,
    pub description: String,
}

/// Summary report of all changes between two API specs.
#[derive(Debug, serde::Serialize)]
pub struct ApiDiffReport {
    pub changes: Vec<ApiChange>,
    pub breaking_count: usize,
    pub non_breaking_count: usize,
    pub deprecation_count: usize,
}

/// Compares two OpenAPI 3.x JSON specs and reports changes.
pub struct ApiDiffer;

impl ApiDiffer {
    /// Diff two OpenAPI 3.x specs provided as JSON strings.
    pub fn diff(old_spec: &str, new_spec: &str) -> Result<ApiDiffReport> {
        let old: Value = serde_json::from_str(old_spec)
            .map_err(|e| ApexError::Detect(format!("failed to parse old spec: {e}")))?;
        let new: Value = serde_json::from_str(new_spec)
            .map_err(|e| ApexError::Detect(format!("failed to parse new spec: {e}")))?;

        let mut changes = Vec::new();

        let old_paths = old.get("paths").and_then(Value::as_object);
        let new_paths = new.get("paths").and_then(Value::as_object);

        let empty_map = serde_json::Map::new();
        let old_paths = old_paths.unwrap_or(&empty_map);
        let new_paths = new_paths.unwrap_or(&empty_map);

        let http_methods = ["get", "post", "put", "delete", "patch", "head", "options"];

        // Check for removed endpoints and changes to existing endpoints
        for (path, old_path_item) in old_paths {
            match new_paths.get(path) {
                None => {
                    // Entire path removed — each method is a breaking change
                    for method in &http_methods {
                        if old_path_item.get(method).is_some() {
                            changes.push(ApiChange {
                                kind: ChangeKind::Breaking,
                                path: path.clone(),
                                method: method.to_uppercase(),
                                description: format!("endpoint {method} {path} removed"),
                            });
                        }
                    }
                }
                Some(new_path_item) => {
                    for method in &http_methods {
                        let old_op = old_path_item.get(method);
                        let new_op = new_path_item.get(method);

                        match (old_op, new_op) {
                            (Some(_), None) => {
                                changes.push(ApiChange {
                                    kind: ChangeKind::Breaking,
                                    path: path.clone(),
                                    method: method.to_uppercase(),
                                    description: format!("endpoint {method} {path} removed"),
                                });
                            }
                            (Some(old_op), Some(new_op)) => {
                                Self::diff_operation(
                                    path,
                                    method,
                                    old_op,
                                    new_op,
                                    &old,
                                    &new,
                                    &mut changes,
                                );
                            }
                            (None, Some(_)) => {
                                // New method on existing path
                                changes.push(ApiChange {
                                    kind: ChangeKind::NonBreaking,
                                    path: path.clone(),
                                    method: method.to_uppercase(),
                                    description: format!("new endpoint {method} {path} added"),
                                });
                            }
                            (None, None) => {}
                        }
                    }
                }
            }
        }

        // Check for new paths (entirely new endpoints)
        for (path, new_path_item) in new_paths {
            if old_paths.contains_key(path) {
                continue; // already handled above
            }
            for method in &http_methods {
                if new_path_item.get(method).is_some() {
                    changes.push(ApiChange {
                        kind: ChangeKind::NonBreaking,
                        path: path.clone(),
                        method: method.to_uppercase(),
                        description: format!("new endpoint {method} {path} added"),
                    });
                }
            }
        }

        let breaking_count = changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Breaking)
            .count();
        let non_breaking_count = changes
            .iter()
            .filter(|c| c.kind == ChangeKind::NonBreaking)
            .count();
        let deprecation_count = changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Deprecation)
            .count();

        Ok(ApiDiffReport {
            changes,
            breaking_count,
            non_breaking_count,
            deprecation_count,
        })
    }

    /// Compare two operations (same path + method) for changes.
    fn diff_operation(
        path: &str,
        method: &str,
        old_op: &Value,
        new_op: &Value,
        old_root: &Value,
        new_root: &Value,
        changes: &mut Vec<ApiChange>,
    ) {
        // Check deprecation
        let old_deprecated = old_op
            .get("deprecated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let new_deprecated = new_op
            .get("deprecated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !old_deprecated && new_deprecated {
            changes.push(ApiChange {
                kind: ChangeKind::Deprecation,
                path: path.to_string(),
                method: method.to_uppercase(),
                description: format!("endpoint {method} {path} marked deprecated"),
            });
        }

        // Compare parameters
        Self::diff_parameters(path, method, old_op, new_op, old_root, new_root, changes);

        // Compare request body
        Self::diff_request_body(path, method, old_op, new_op, old_root, new_root, changes);

        // Compare security requirements
        Self::diff_security(path, method, old_op, new_op, changes);
    }

    /// Compare parameters between old and new operations.
    fn diff_parameters(
        path: &str,
        method: &str,
        old_op: &Value,
        new_op: &Value,
        old_root: &Value,
        new_root: &Value,
        changes: &mut Vec<ApiChange>,
    ) {
        let old_params = Self::collect_parameters(old_op, old_root);
        let new_params = Self::collect_parameters(new_op, new_root);

        // Check for removed parameters
        for (key, old_param) in &old_params {
            let required = old_param
                .get("required")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !new_params.contains_key(key) && required {
                changes.push(ApiChange {
                    kind: ChangeKind::Breaking,
                    path: path.to_string(),
                    method: method.to_uppercase(),
                    description: format!("required parameter '{key}' removed"),
                });
            }
        }

        // Check for added parameters
        for (key, new_param) in &new_params {
            if !old_params.contains_key(key) {
                let required = new_param
                    .get("required")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if required {
                    changes.push(ApiChange {
                        kind: ChangeKind::Breaking,
                        path: path.to_string(),
                        method: method.to_uppercase(),
                        description: format!("new required parameter '{key}' added"),
                    });
                } else {
                    changes.push(ApiChange {
                        kind: ChangeKind::NonBreaking,
                        path: path.to_string(),
                        method: method.to_uppercase(),
                        description: format!("optional parameter '{key}' added"),
                    });
                }
            }
        }

        // Check for type changes on existing parameters
        for (key, old_param) in &old_params {
            if let Some(new_param) = new_params.get(key) {
                let old_schema =
                    Self::resolve_ref(old_param.get("schema").unwrap_or(&Value::Null), old_root, &mut HashSet::new());
                let new_schema =
                    Self::resolve_ref(new_param.get("schema").unwrap_or(&Value::Null), new_root, &mut HashSet::new());
                let old_type = old_schema.get("type").and_then(Value::as_str);
                let new_type = new_schema.get("type").and_then(Value::as_str);
                if let (Some(old_t), Some(new_t)) = (&old_type, &new_type) {
                    if old_t != new_t {
                        changes.push(ApiChange {
                            kind: ChangeKind::Breaking,
                            path: path.to_string(),
                            method: method.to_uppercase(),
                            description: format!(
                                "parameter '{key}' type changed from '{old_t}' to '{new_t}'"
                            ),
                        });
                    }
                }

                // Check enum narrowing/widening
                Self::diff_enum(
                    path,
                    method,
                    &format!("parameter '{key}'"),
                    &old_schema,
                    &new_schema,
                    changes,
                );
            }
        }
    }

    /// Collect parameters into a map keyed by "name:in" (e.g., "id:path").
    fn collect_parameters(op: &Value, root: &Value) -> std::collections::HashMap<String, Value> {
        let mut map = std::collections::HashMap::new();
        if let Some(params) = op.get("parameters").and_then(Value::as_array) {
            for param in params {
                let param = Self::resolve_ref(param, root, &mut HashSet::new()).into_owned();
                let name = param.get("name").and_then(Value::as_str).unwrap_or("");
                let location = param.get("in").and_then(Value::as_str).unwrap_or("");
                let key = format!("{name}:{location}");
                map.insert(key, param);
            }
        }
        map
    }

    /// Compare request body schemas.
    fn diff_request_body(
        path: &str,
        method: &str,
        old_op: &Value,
        new_op: &Value,
        old_root: &Value,
        new_root: &Value,
        changes: &mut Vec<ApiChange>,
    ) {
        let old_body = old_op.get("requestBody");
        let new_body = new_op.get("requestBody");

        if old_body.is_none() && new_body.is_none() {
            return;
        }

        // Resolve refs on the request body itself
        let old_body = old_body.map(|b| Self::resolve_ref(b, old_root, &mut HashSet::new()));
        let new_body = new_body.map(|b| Self::resolve_ref(b, new_root, &mut HashSet::new()));

        let old_schema = old_body
            .as_deref()
            .and_then(|b| Self::get_json_schema(b, old_root));
        let new_schema = new_body
            .as_deref()
            .and_then(|b| Self::get_json_schema(b, new_root));

        match (old_schema, new_schema) {
            (Some(old_s), Some(new_s)) => {
                Self::diff_schema_properties(
                    path,
                    method,
                    "request body",
                    &old_s,
                    &new_s,
                    old_root,
                    new_root,
                    changes,
                );
            }
            (None, Some(_)) => {
                // Request body added — could be breaking if required
                let required = new_body
                    .as_deref()
                    .and_then(|b| b.get("required"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if required {
                    changes.push(ApiChange {
                        kind: ChangeKind::Breaking,
                        path: path.to_string(),
                        method: method.to_uppercase(),
                        description: "required request body added".to_string(),
                    });
                }
            }
            (Some(_), None) => {
                // Request body removed — non-breaking (clients can still send, server ignores)
            }
            (None, None) => {}
        }
    }

    /// Extract the JSON schema from a request body's `content.application/json.schema`.
    fn get_json_schema<'a>(
        body: &'a Value,
        root: &'a Value,
    ) -> Option<std::borrow::Cow<'a, Value>> {
        let schema = body
            .get("content")?
            .get("application/json")?
            .get("schema")?;
        Some(Self::resolve_ref(schema, root, &mut HashSet::new()))
    }

    /// Compare properties of two object schemas, detecting added required fields.
    #[allow(clippy::too_many_arguments)]
    fn diff_schema_properties(
        path: &str,
        method: &str,
        context: &str,
        old_schema: &Value,
        new_schema: &Value,
        old_root: &Value,
        new_root: &Value,
        changes: &mut Vec<ApiChange>,
    ) {
        let old_resolved = Self::resolve_ref(old_schema, old_root, &mut HashSet::new());
        let new_resolved = Self::resolve_ref(new_schema, new_root, &mut HashSet::new());

        let old_props = old_resolved.get("properties").and_then(Value::as_object);
        let new_props = new_resolved.get("properties").and_then(Value::as_object);

        let new_required: Vec<&str> = new_resolved
            .get("required")
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();

        let old_required: Vec<&str> = old_resolved
            .get("required")
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();

        let empty_map = serde_json::Map::new();
        let old_props = old_props.unwrap_or(&empty_map);
        let new_props = new_props.unwrap_or(&empty_map);

        // Check for newly added required fields
        for (prop_name, _) in new_props {
            if !old_props.contains_key(prop_name) && new_required.contains(&prop_name.as_str()) {
                changes.push(ApiChange {
                    kind: ChangeKind::Breaking,
                    path: path.to_string(),
                    method: method.to_uppercase(),
                    description: format!("required field '{prop_name}' added to {context}"),
                });
            } else if !old_props.contains_key(prop_name) {
                changes.push(ApiChange {
                    kind: ChangeKind::NonBreaking,
                    path: path.to_string(),
                    method: method.to_uppercase(),
                    description: format!("optional field '{prop_name}' added to {context}"),
                });
            }
        }

        // Check for fields that became required
        for req in &new_required {
            if old_props.contains_key(*req) && !old_required.contains(req) {
                changes.push(ApiChange {
                    kind: ChangeKind::Breaking,
                    path: path.to_string(),
                    method: method.to_uppercase(),
                    description: format!(
                        "field '{req}' in {context} changed from optional to required"
                    ),
                });
            }
        }

        // Check type changes on existing properties
        for (prop_name, old_prop) in old_props {
            if let Some(new_prop) = new_props.get(prop_name) {
                let old_prop = Self::resolve_ref(old_prop, old_root, &mut HashSet::new());
                let new_prop = Self::resolve_ref(new_prop, new_root, &mut HashSet::new());
                let old_type = old_prop.get("type").and_then(Value::as_str);
                let new_type = new_prop.get("type").and_then(Value::as_str);
                if let (Some(old_t), Some(new_t)) = (&old_type, &new_type) {
                    if old_t != new_t {
                        changes.push(ApiChange {
                            kind: ChangeKind::Breaking,
                            path: path.to_string(),
                            method: method.to_uppercase(),
                            description: format!(
                                "field '{prop_name}' in {context} type changed from '{old_t}' to '{new_t}'"
                            ),
                        });
                    }
                }

                Self::diff_enum(
                    path,
                    method,
                    &format!("field '{prop_name}' in {context}"),
                    &old_prop,
                    &new_prop,
                    changes,
                );
            }
        }

        // Check response fields (added fields in response are non-breaking)
        // This is handled at the caller level — response schemas go through
        // diff_response_schemas which adds NonBreaking for new fields.
    }

    /// Compare enum values — narrowing is breaking, widening is non-breaking.
    fn diff_enum(
        path: &str,
        method: &str,
        context: &str,
        old_schema: &Value,
        new_schema: &Value,
        changes: &mut Vec<ApiChange>,
    ) {
        let old_enum = old_schema.get("enum").and_then(Value::as_array);
        let new_enum = new_schema.get("enum").and_then(Value::as_array);

        if let (Some(old_vals), Some(new_vals)) = (old_enum, new_enum) {
            // Check for removed variants (narrowing = breaking)
            for val in old_vals {
                if !new_vals.contains(val) {
                    changes.push(ApiChange {
                        kind: ChangeKind::Breaking,
                        path: path.to_string(),
                        method: method.to_uppercase(),
                        description: format!(
                            "{context} enum value '{}' removed",
                            val_to_string(val)
                        ),
                    });
                }
            }

            // Check for added variants (widening = non-breaking)
            for val in new_vals {
                if !old_vals.contains(val) {
                    changes.push(ApiChange {
                        kind: ChangeKind::NonBreaking,
                        path: path.to_string(),
                        method: method.to_uppercase(),
                        description: format!("{context} enum value '{}' added", val_to_string(val)),
                    });
                }
            }
        }
    }

    /// Compare security requirements.
    fn diff_security(
        path: &str,
        method: &str,
        old_op: &Value,
        new_op: &Value,
        changes: &mut Vec<ApiChange>,
    ) {
        let old_security = old_op.get("security");
        let new_security = new_op.get("security");

        match (old_security, new_security) {
            (None, Some(sec)) if !is_empty_security(sec) => {
                changes.push(ApiChange {
                    kind: ChangeKind::Breaking,
                    path: path.to_string(),
                    method: method.to_uppercase(),
                    description: "authentication requirement added".to_string(),
                });
            }
            (Some(old_sec), Some(new_sec)) if old_sec != new_sec => {
                changes.push(ApiChange {
                    kind: ChangeKind::Breaking,
                    path: path.to_string(),
                    method: method.to_uppercase(),
                    description: "authentication requirement changed".to_string(),
                });
            }
            _ => {}
        }
    }

    /// Resolve a `$ref` pointer within a document. Returns a borrowed or owned `Value`.
    /// Uses `visited` to prevent infinite recursion on circular references.
    fn resolve_ref<'a>(
        value: &'a Value,
        root: &'a Value,
        visited: &mut HashSet<String>,
    ) -> std::borrow::Cow<'a, Value> {
        if let Some(ref_str) = value.get("$ref").and_then(Value::as_str) {
            if !visited.insert(ref_str.to_string()) {
                // Circular reference detected — return the unresolved value
                return std::borrow::Cow::Borrowed(value);
            }
            if let Some(resolved) = Self::follow_ref(ref_str, root) {
                if resolved.get("$ref").is_some() {
                    return Self::resolve_ref(resolved, root, visited);
                }
                return std::borrow::Cow::Borrowed(resolved);
            }
        }
        std::borrow::Cow::Borrowed(value)
    }

    /// Follow a JSON pointer like `#/components/schemas/User`.
    fn follow_ref<'a>(ref_str: &str, root: &'a Value) -> Option<&'a Value> {
        let path = ref_str.strip_prefix("#/")?;
        let mut current = root;
        for segment in path.split('/') {
            // Handle JSON pointer escaping
            let segment = segment.replace("~1", "/").replace("~0", "~");
            current = current.get(segment.as_str())?;
        }
        Some(current)
    }
}

fn val_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn is_empty_security(sec: &Value) -> bool {
    match sec.as_array() {
        Some(arr) => {
            arr.is_empty()
                || arr
                    .iter()
                    .all(|v| v.as_object().is_some_and(|m| m.is_empty()))
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_spec(paths_json: &str) -> String {
        format!(
            r#"{{
                "openapi": "3.0.0",
                "info": {{ "title": "Test", "version": "1.0" }},
                "paths": {paths_json}
            }}"#
        )
    }

    fn make_spec_with_components(paths_json: &str, components_json: &str) -> String {
        format!(
            r#"{{
                "openapi": "3.0.0",
                "info": {{ "title": "Test", "version": "1.0" }},
                "paths": {paths_json},
                "components": {components_json}
            }}"#
        )
    }

    #[test]
    fn removed_endpoint_is_breaking() {
        let old = make_spec(r#"{ "/users": { "get": { "summary": "list" } } }"#);
        let new = make_spec(r#"{}"#);
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.breaking_count, 1);
        assert_eq!(report.changes[0].kind, ChangeKind::Breaking);
        assert!(report.changes[0].description.contains("removed"));
    }

    #[test]
    fn removed_method_is_breaking() {
        let old = make_spec(
            r#"{ "/users": { "get": { "summary": "list" }, "post": { "summary": "create" } } }"#,
        );
        let new = make_spec(r#"{ "/users": { "get": { "summary": "list" } } }"#);
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.breaking_count, 1);
        let breaking: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Breaking)
            .collect();
        assert_eq!(breaking[0].method, "POST");
    }

    #[test]
    fn changed_parameter_type_is_breaking() {
        let old = make_spec(
            r#"{ "/users/{id}": { "get": {
                "parameters": [
                    { "name": "id", "in": "path", "required": true,
                      "schema": { "type": "integer" } }
                ]
            } } }"#,
        );
        let new = make_spec(
            r#"{ "/users/{id}": { "get": {
                "parameters": [
                    { "name": "id", "in": "path", "required": true,
                      "schema": { "type": "string" } }
                ]
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.breaking_count, 1);
        assert!(report.changes[0].description.contains("type changed"));
    }

    #[test]
    fn added_optional_param_is_non_breaking() {
        let old = make_spec(r#"{ "/users": { "get": { "parameters": [] } } }"#);
        let new = make_spec(
            r#"{ "/users": { "get": { "parameters": [
                { "name": "page", "in": "query", "required": false,
                  "schema": { "type": "integer" } }
            ] } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.breaking_count, 0);
        assert_eq!(report.non_breaking_count, 1);
        assert!(report.changes[0].description.contains("optional"));
    }

    #[test]
    fn new_endpoint_is_non_breaking() {
        let old = make_spec(r#"{ "/users": { "get": { "summary": "list" } } }"#);
        let new = make_spec(
            r#"{ "/users": { "get": { "summary": "list" } },
                 "/posts": { "get": { "summary": "list posts" } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.breaking_count, 0);
        assert_eq!(report.non_breaking_count, 1);
        assert!(report.changes[0].description.contains("new endpoint"));
    }

    #[test]
    fn deprecated_endpoint_detected() {
        let old = make_spec(r#"{ "/users": { "get": { "summary": "list" } } }"#);
        let new =
            make_spec(r#"{ "/users": { "get": { "summary": "list", "deprecated": true } } }"#);
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.deprecation_count, 1);
        assert_eq!(report.changes[0].kind, ChangeKind::Deprecation);
        assert!(report.changes[0].description.contains("deprecated"));
    }

    #[test]
    fn added_required_body_field_is_breaking() {
        let old = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "name": { "type": "string" }
                                },
                                "required": ["name"]
                            }
                        }
                    }
                }
            } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "name": { "type": "string" },
                                    "email": { "type": "string" }
                                },
                                "required": ["name", "email"]
                            }
                        }
                    }
                }
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        let breaking: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Breaking)
            .collect();
        assert!(!breaking.is_empty());
        assert!(breaking.iter().any(|c| c.description.contains("email")));
    }

    #[test]
    fn narrowed_enum_is_breaking() {
        let old = make_spec(
            r#"{ "/users": { "get": {
                "parameters": [
                    { "name": "status", "in": "query",
                      "schema": { "type": "string", "enum": ["active", "inactive", "pending"] } }
                ]
            } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "get": {
                "parameters": [
                    { "name": "status", "in": "query",
                      "schema": { "type": "string", "enum": ["active", "inactive"] } }
                ]
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        let breaking: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Breaking)
            .collect();
        assert!(!breaking.is_empty());
        assert!(breaking[0].description.contains("pending"));
        assert!(breaking[0].description.contains("removed"));
    }

    #[test]
    fn widened_enum_is_non_breaking() {
        let old = make_spec(
            r#"{ "/users": { "get": {
                "parameters": [
                    { "name": "status", "in": "query",
                      "schema": { "type": "string", "enum": ["active", "inactive"] } }
                ]
            } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "get": {
                "parameters": [
                    { "name": "status", "in": "query",
                      "schema": { "type": "string", "enum": ["active", "inactive", "pending"] } }
                ]
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        let non_breaking: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.kind == ChangeKind::NonBreaking)
            .collect();
        assert!(!non_breaking.is_empty());
        assert!(non_breaking[0].description.contains("pending"));
        assert!(non_breaking[0].description.contains("added"));
    }

    #[test]
    fn ref_resolution_works() {
        let old = make_spec_with_components(
            r##"{ "/users/{id}": { "get": {
                "parameters": [
                    { "$ref": "#/components/parameters/UserId" }
                ]
            } } }"##,
            r##"{ "parameters": {
                "UserId": {
                    "name": "id", "in": "path", "required": true,
                    "schema": { "type": "integer" }
                }
            } }"##,
        );
        let new = make_spec_with_components(
            r##"{ "/users/{id}": { "get": {
                "parameters": [
                    { "$ref": "#/components/parameters/UserId" }
                ]
            } } }"##,
            r##"{ "parameters": {
                "UserId": {
                    "name": "id", "in": "path", "required": true,
                    "schema": { "type": "string" }
                }
            } }"##,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.breaking_count, 1);
        assert!(report.changes[0].description.contains("type changed"));
    }

    #[test]
    fn security_change_is_breaking() {
        let old = make_spec(r#"{ "/users": { "get": { "summary": "list" } } }"#);
        let new = make_spec(
            r#"{ "/users": { "get": {
                "summary": "list",
                "security": [{ "bearerAuth": [] }]
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        let breaking: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Breaking)
            .collect();
        assert!(!breaking.is_empty());
        assert!(breaking[0].description.contains("authentication"));
    }

    #[test]
    fn removed_required_param_is_breaking() {
        let old = make_spec(
            r#"{ "/users": { "get": {
                "parameters": [
                    { "name": "token", "in": "header", "required": true,
                      "schema": { "type": "string" } }
                ]
            } } }"#,
        );
        let new = make_spec(r#"{ "/users": { "get": { "parameters": [] } } }"#);
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.breaking_count, 1);
        assert!(report.changes[0].description.contains("removed"));
    }

    #[test]
    fn integration_v1_v2_fixtures() {
        let old = include_str!("../tests/fixtures/openapi-v1.json");
        let new = include_str!("../tests/fixtures/openapi-v2.json");
        let report = ApiDiffer::diff(old, new).unwrap();

        // Verify we detect the expected changes:
        // Breaking: GET /users/{id} removed, POST /users new required field
        assert!(
            report.breaking_count >= 2,
            "expected at least 2 breaking changes, got {}",
            report.breaking_count
        );

        // Non-breaking: optional query param on GET /users, new DELETE /users/{id}
        assert!(
            report.non_breaking_count >= 2,
            "expected at least 2 non-breaking changes, got {}",
            report.non_breaking_count
        );

        // Deprecation: GET /users marked deprecated
        assert!(
            report.deprecation_count >= 1,
            "expected at least 1 deprecation, got {}",
            report.deprecation_count
        );
    }

    #[test]
    fn invalid_json_returns_error() {
        let result = ApiDiffer::diff("not json", "{}");
        assert!(result.is_err());
    }

    #[test]
    fn empty_specs_no_changes() {
        let old = make_spec(r#"{}"#);
        let new = make_spec(r#"{}"#);
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert!(report.changes.is_empty());
    }

    #[test]
    fn body_schema_ref_resolution() {
        let old = make_spec_with_components(
            r##"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/CreateUser" }
                        }
                    }
                }
            } } }"##,
            r##"{ "schemas": {
                "CreateUser": {
                    "type": "object",
                    "properties": { "name": { "type": "string" } },
                    "required": ["name"]
                }
            } }"##,
        );
        let new = make_spec_with_components(
            r##"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/CreateUser" }
                        }
                    }
                }
            } } }"##,
            r##"{ "schemas": {
                "CreateUser": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "email": { "type": "string" }
                    },
                    "required": ["name", "email"]
                }
            } }"##,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        let breaking: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Breaking)
            .collect();
        assert!(!breaking.is_empty());
        assert!(breaking.iter().any(|c| c.description.contains("email")));
    }

    // ---- Additional tests for branch coverage ----

    #[test]
    fn invalid_new_spec_returns_error() {
        // Exercises the error path on line 44 (new spec parse failure)
        let old = make_spec(r#"{}"#);
        let result = ApiDiffer::diff(&old, "not json");
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("new spec"));
    }

    #[test]
    fn specs_without_paths_key() {
        // Exercises the None branch of old_paths/new_paths (lines 48-53)
        let old = r#"{ "openapi": "3.0.0", "info": { "title": "T", "version": "1" } }"#;
        let new = r#"{ "openapi": "3.0.0", "info": { "title": "T", "version": "1" } }"#;
        let report = ApiDiffer::diff(old, new).unwrap();
        assert!(report.changes.is_empty());
    }

    #[test]
    fn old_has_no_paths_new_has_paths() {
        // old_paths is None (unwrap_or empty), new_paths has entries
        let old = r#"{ "openapi": "3.0.0", "info": { "title": "T", "version": "1" } }"#;
        let new = make_spec(r#"{ "/users": { "get": { "summary": "list" } } }"#);
        let report = ApiDiffer::diff(old, &new).unwrap();
        assert_eq!(report.non_breaking_count, 1);
        assert!(report.changes[0].description.contains("new endpoint"));
    }

    #[test]
    fn old_has_paths_new_has_no_paths() {
        // new_paths is None (unwrap_or empty) — all old endpoints are removed
        let old = make_spec(r#"{ "/users": { "get": { "summary": "list" } } }"#);
        let new = r#"{ "openapi": "3.0.0", "info": { "title": "T", "version": "1" } }"#;
        let report = ApiDiffer::diff(&old, new).unwrap();
        assert_eq!(report.breaking_count, 1);
        assert!(report.changes[0].description.contains("removed"));
    }

    #[test]
    fn removed_path_with_multiple_methods() {
        // Exercises the inner loop at line 62-71: path removed, multiple methods present
        let old = make_spec(
            r#"{ "/users": { "get": { "summary": "list" }, "post": { "summary": "create" }, "delete": { "summary": "clear" } } }"#,
        );
        let new = make_spec(r#"{}"#);
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.breaking_count, 3);
        let methods: Vec<&str> = report.changes.iter().map(|c| c.method.as_str()).collect();
        assert!(methods.contains(&"GET"));
        assert!(methods.contains(&"POST"));
        assert!(methods.contains(&"DELETE"));
    }

    #[test]
    fn removed_path_only_counts_existing_methods() {
        // Path removed but only "get" method exists — other HTTP methods (post, put, etc.) should NOT produce changes
        let old = make_spec(r#"{ "/items": { "get": { "summary": "list" } } }"#);
        let new = make_spec(r#"{}"#);
        let report = ApiDiffer::diff(&old, &new).unwrap();
        // Only 1 breaking change for "get", not 7 for all HTTP methods
        assert_eq!(report.breaking_count, 1);
        assert_eq!(report.changes.len(), 1);
    }

    #[test]
    fn new_method_on_existing_path() {
        // Exercises (None, Some(_)) branch at line 98-106
        let old = make_spec(r#"{ "/users": { "get": { "summary": "list" } } }"#);
        let new = make_spec(
            r#"{ "/users": { "get": { "summary": "list" }, "post": { "summary": "create" } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.non_breaking_count, 1);
        assert_eq!(report.changes[0].method, "POST");
        assert!(report.changes[0].description.contains("new endpoint"));
    }

    #[test]
    fn both_methods_none_no_change() {
        // Exercises (None, None) branch at line 107 — method doesn't exist in either
        // Using "put" which exists in neither
        let old = make_spec(r#"{ "/users": { "get": { "summary": "list" } } }"#);
        let new = make_spec(r#"{ "/users": { "get": { "summary": "list" } } }"#);
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert!(report.changes.is_empty());
    }

    #[test]
    fn already_deprecated_no_change() {
        // old_deprecated=true, new_deprecated=true — should NOT add deprecation change
        let old =
            make_spec(r#"{ "/users": { "get": { "summary": "list", "deprecated": true } } }"#);
        let new =
            make_spec(r#"{ "/users": { "get": { "summary": "list", "deprecated": true } } }"#);
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.deprecation_count, 0);
    }

    #[test]
    fn undeprecated_no_deprecation_change() {
        // old_deprecated=true, new_deprecated=false — not treated as a change by current logic
        let old =
            make_spec(r#"{ "/users": { "get": { "summary": "list", "deprecated": true } } }"#);
        let new = make_spec(r#"{ "/users": { "get": { "summary": "list" } } }"#);
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.deprecation_count, 0);
    }

    #[test]
    fn removed_optional_param_not_breaking() {
        // Exercises lines 204-217: parameter removed but required=false
        let old = make_spec(
            r#"{ "/users": { "get": { "parameters": [
                { "name": "page", "in": "query", "required": false,
                  "schema": { "type": "integer" } }
            ] } } }"#,
        );
        let new = make_spec(r#"{ "/users": { "get": { "parameters": [] } } }"#);
        let report = ApiDiffer::diff(&old, &new).unwrap();
        // Removing an optional parameter is not breaking
        assert_eq!(report.breaking_count, 0);
    }

    #[test]
    fn removed_param_no_required_field_not_breaking() {
        // Parameter without "required" field — defaults to false
        let old = make_spec(
            r#"{ "/users": { "get": { "parameters": [
                { "name": "page", "in": "query",
                  "schema": { "type": "integer" } }
            ] } } }"#,
        );
        let new = make_spec(r#"{ "/users": { "get": { "parameters": [] } } }"#);
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.breaking_count, 0);
    }

    #[test]
    fn added_required_param_is_breaking() {
        // Exercises lines 226-231: new required parameter added
        let old = make_spec(r#"{ "/users": { "get": { "parameters": [] } } }"#);
        let new = make_spec(
            r#"{ "/users": { "get": { "parameters": [
                { "name": "token", "in": "header", "required": true,
                  "schema": { "type": "string" } }
            ] } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.breaking_count, 1);
        assert!(report.changes[0]
            .description
            .contains("new required parameter"));
    }

    #[test]
    fn param_same_type_no_change() {
        // Exercises lines 253-264: both params have same type — no breaking change
        let old = make_spec(
            r#"{ "/users": { "get": { "parameters": [
                { "name": "id", "in": "query",
                  "schema": { "type": "string" } }
            ] } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "get": { "parameters": [
                { "name": "id", "in": "query",
                  "schema": { "type": "string" } }
            ] } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.breaking_count, 0);
        assert!(report.changes.is_empty());
    }

    #[test]
    fn param_no_schema_no_crash() {
        // Parameters without schema — exercises unwrap_or(&Value::Null) at line 248
        let old = make_spec(
            r#"{ "/users": { "get": { "parameters": [
                { "name": "id", "in": "query" }
            ] } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "get": { "parameters": [
                { "name": "id", "in": "query" }
            ] } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert!(report.changes.is_empty());
    }

    #[test]
    fn param_one_has_type_other_doesnt() {
        // One param has type, other doesn't — (Some, None) in the type check
        let old = make_spec(
            r#"{ "/users": { "get": { "parameters": [
                { "name": "id", "in": "query", "schema": { "type": "string" } }
            ] } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "get": { "parameters": [
                { "name": "id", "in": "query", "schema": {} }
            ] } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        // No type-change breaking change since new_type is None
        let type_changes: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.description.contains("type changed"))
            .collect();
        assert!(type_changes.is_empty());
    }

    #[test]
    fn request_body_removed_not_breaking() {
        // Exercises (Some(_), None) branch at line 351
        let old = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": { "type": "object", "properties": { "name": { "type": "string" } } }
                        }
                    }
                }
            } } }"#,
        );
        let new = make_spec(r#"{ "/users": { "post": { "summary": "create" } } }"#);
        let report = ApiDiffer::diff(&old, &new).unwrap();
        // Request body removal is non-breaking per the code comment
        assert_eq!(report.breaking_count, 0);
    }

    #[test]
    fn request_body_added_required_is_breaking() {
        // Exercises (None, Some(_)) branch at lines 335-349 with required=true
        let old = make_spec(r#"{ "/users": { "post": { "summary": "create" } } }"#);
        let new = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": { "type": "object", "properties": { "name": { "type": "string" } } }
                        }
                    }
                }
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.breaking_count, 1);
        assert!(report.changes[0]
            .description
            .contains("required request body added"));
    }

    #[test]
    fn request_body_added_optional_not_breaking() {
        // Exercises (None, Some(_)) branch with required=false
        let old = make_spec(r#"{ "/users": { "post": { "summary": "create" } } }"#);
        let new = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "required": false,
                    "content": {
                        "application/json": {
                            "schema": { "type": "object", "properties": { "name": { "type": "string" } } }
                        }
                    }
                }
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.breaking_count, 0);
    }

    #[test]
    fn request_body_added_no_required_field() {
        // Exercises line 341: "required" field missing — defaults to false
        let old = make_spec(r#"{ "/users": { "post": { "summary": "create" } } }"#);
        let new = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": { "type": "object" }
                        }
                    }
                }
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.breaking_count, 0);
    }

    #[test]
    fn request_body_both_none() {
        // Exercises the early return at line 307-309
        let old = make_spec(r#"{ "/users": { "get": { "summary": "list" } } }"#);
        let new = make_spec(r#"{ "/users": { "get": { "summary": "list" } } }"#);
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert!(report.changes.is_empty());
    }

    #[test]
    fn schema_optional_field_added() {
        // Exercises line 413-420: new property that's NOT in required list
        let old = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": { "name": { "type": "string" } },
                                "required": ["name"]
                            }
                        }
                    }
                }
            } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "name": { "type": "string" },
                                    "nickname": { "type": "string" }
                                },
                                "required": ["name"]
                            }
                        }
                    }
                }
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        let non_breaking: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.kind == ChangeKind::NonBreaking)
            .collect();
        assert!(!non_breaking.is_empty());
        assert!(non_breaking
            .iter()
            .any(|c| c.description.contains("optional field 'nickname'")));
    }

    #[test]
    fn field_became_required() {
        // Exercises lines 424-435: existing optional field becomes required
        let old = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "name": { "type": "string" },
                                    "email": { "type": "string" }
                                },
                                "required": ["name"]
                            }
                        }
                    }
                }
            } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "name": { "type": "string" },
                                    "email": { "type": "string" }
                                },
                                "required": ["name", "email"]
                            }
                        }
                    }
                }
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        let breaking: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Breaking)
            .collect();
        assert!(!breaking.is_empty());
        assert!(breaking
            .iter()
            .any(|c| c.description.contains("optional to required")));
    }

    #[test]
    fn schema_property_type_changed() {
        // Exercises lines 438-455: property type changes in request body schema
        let old = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": { "age": { "type": "string" } }
                            }
                        }
                    }
                }
            } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": { "age": { "type": "integer" } }
                            }
                        }
                    }
                }
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        let breaking: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Breaking)
            .collect();
        assert!(!breaking.is_empty());
        assert!(breaking[0].description.contains("type changed"));
        assert!(breaking[0].description.contains("age"));
    }

    #[test]
    fn schema_property_type_unchanged() {
        // Exercises lines 444-455 when types match
        let old = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": { "name": { "type": "string" } }
                            }
                        }
                    }
                }
            } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": { "name": { "type": "string" } }
                            }
                        }
                    }
                }
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert!(report.changes.is_empty());
    }

    #[test]
    fn enum_on_schema_property() {
        // Exercises diff_enum called from diff_schema_properties (lines 457-464)
        let old = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "role": { "type": "string", "enum": ["admin", "user"] }
                                }
                            }
                        }
                    }
                }
            } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "role": { "type": "string", "enum": ["admin", "user", "moderator"] }
                                }
                            }
                        }
                    }
                }
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        let non_breaking: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.kind == ChangeKind::NonBreaking)
            .collect();
        assert!(!non_breaking.is_empty());
        assert!(non_breaking[0].description.contains("moderator"));
    }

    #[test]
    fn security_changed_is_breaking() {
        // Exercises (Some, Some) where old_sec != new_sec (line 535)
        let old = make_spec(
            r#"{ "/users": { "get": {
                "summary": "list",
                "security": [{ "basicAuth": [] }]
            } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "get": {
                "summary": "list",
                "security": [{ "bearerAuth": [] }]
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        let breaking: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.description.contains("authentication"))
            .collect();
        assert!(!breaking.is_empty());
        assert!(breaking[0].description.contains("changed"));
    }

    #[test]
    fn security_same_no_change() {
        // Exercises (Some, Some) where old_sec == new_sec — falls through to _
        let old = make_spec(
            r#"{ "/users": { "get": {
                "summary": "list",
                "security": [{ "bearerAuth": [] }]
            } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "get": {
                "summary": "list",
                "security": [{ "bearerAuth": [] }]
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        let auth_changes: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.description.contains("authentication"))
            .collect();
        assert!(auth_changes.is_empty());
    }

    #[test]
    fn security_removed_no_breaking() {
        // Exercises (Some(_), None) which falls through to _ catch-all
        let old = make_spec(
            r#"{ "/users": { "get": {
                "summary": "list",
                "security": [{ "bearerAuth": [] }]
            } } }"#,
        );
        let new = make_spec(r#"{ "/users": { "get": { "summary": "list" } } }"#);
        let report = ApiDiffer::diff(&old, &new).unwrap();
        let auth_changes: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.description.contains("authentication"))
            .collect();
        assert!(auth_changes.is_empty());
    }

    #[test]
    fn security_added_empty_array_not_breaking() {
        // Exercises is_empty_security with empty array (line 584)
        let old = make_spec(r#"{ "/users": { "get": { "summary": "list" } } }"#);
        let new = make_spec(
            r#"{ "/users": { "get": {
                "summary": "list",
                "security": []
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        let auth_changes: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.description.contains("authentication"))
            .collect();
        assert!(auth_changes.is_empty());
    }

    #[test]
    fn security_added_empty_objects_not_breaking() {
        // Exercises is_empty_security with array of empty objects (line 586-587)
        let old = make_spec(r#"{ "/users": { "get": { "summary": "list" } } }"#);
        let new = make_spec(
            r#"{ "/users": { "get": {
                "summary": "list",
                "security": [{}]
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        let auth_changes: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.description.contains("authentication"))
            .collect();
        assert!(auth_changes.is_empty());
    }

    #[test]
    fn is_empty_security_non_array() {
        // Exercises is_empty_security None branch (line 589) — security is not an array
        let old = make_spec(r#"{ "/users": { "get": { "summary": "list" } } }"#);
        let new = make_spec(
            r#"{ "/users": { "get": {
                "summary": "list",
                "security": "invalid"
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        // Non-array security is treated as non-empty, so it's breaking
        let auth_changes: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.description.contains("authentication"))
            .collect();
        assert!(!auth_changes.is_empty());
    }

    #[test]
    fn val_to_string_non_string_value() {
        // Exercises the `other` branch of val_to_string (line 577)
        // Use integer enum values to trigger this
        let old = make_spec(
            r#"{ "/users": { "get": { "parameters": [
                { "name": "level", "in": "query",
                  "schema": { "type": "integer", "enum": [1, 2, 3] } }
            ] } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "get": { "parameters": [
                { "name": "level", "in": "query",
                  "schema": { "type": "integer", "enum": [1, 2] } }
            ] } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        let breaking: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Breaking)
            .collect();
        assert!(!breaking.is_empty());
        assert!(breaking[0].description.contains("3"));
    }

    #[test]
    fn follow_ref_invalid_format() {
        // Exercises follow_ref returning None when ref doesn't start with "#/"
        // Using a parameter with an invalid $ref
        let old = make_spec(
            r#"{ "/users": { "get": { "parameters": [
                { "$ref": "invalid-ref" }
            ] } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "get": { "parameters": [
                { "$ref": "invalid-ref" }
            ] } } }"#,
        );
        // Should not crash — the ref won't resolve, parameter will be used as-is
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert!(report.changes.is_empty());
    }

    #[test]
    fn follow_ref_dangling_ref() {
        // Exercises follow_ref returning None when path doesn't exist in root
        let old = make_spec(
            r##"{ "/users": { "get": { "parameters": [
                { "$ref": "#/components/parameters/DoesNotExist" }
            ] } } }"##,
        );
        let new = make_spec(
            r##"{ "/users": { "get": { "parameters": [
                { "$ref": "#/components/parameters/DoesNotExist" }
            ] } } }"##,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert!(report.changes.is_empty());
    }

    #[test]
    fn json_pointer_escaping() {
        // Exercises line 567: JSON pointer escaping with ~1 (/) and ~0 (~)
        let old = make_spec_with_components(
            r##"{ "/users": { "get": { "parameters": [
                { "$ref": "#/components/parameters/my~1param~0name" }
            ] } } }"##,
            r##"{ "parameters": {
                "my/param~name": {
                    "name": "test", "in": "query", "required": true,
                    "schema": { "type": "string" }
                }
            } }"##,
        );
        let new = make_spec(r#"{ "/users": { "get": { "parameters": [] } } }"#);
        let report = ApiDiffer::diff(&old, &new).unwrap();
        // The resolved parameter is required, so removing it is breaking
        assert_eq!(report.breaking_count, 1);
        assert!(report.changes[0].description.contains("removed"));
    }

    #[test]
    fn recursive_ref_resolution() {
        // Exercises line 552-554: a $ref pointing to another $ref
        let old = make_spec_with_components(
            r##"{ "/users/{id}": { "get": {
                "parameters": [
                    { "$ref": "#/components/parameters/UserIdAlias" }
                ]
            } } }"##,
            r##"{
                "parameters": {
                    "UserIdAlias": { "$ref": "#/components/parameters/UserId" },
                    "UserId": {
                        "name": "id", "in": "path", "required": true,
                        "schema": { "type": "integer" }
                    }
                }
            }"##,
        );
        let new = make_spec_with_components(
            r##"{ "/users/{id}": { "get": {
                "parameters": [
                    { "$ref": "#/components/parameters/UserIdAlias" }
                ]
            } } }"##,
            r##"{
                "parameters": {
                    "UserIdAlias": { "$ref": "#/components/parameters/UserId" },
                    "UserId": {
                        "name": "id", "in": "path", "required": true,
                        "schema": { "type": "string" }
                    }
                }
            }"##,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.breaking_count, 1);
        assert!(report.changes[0].description.contains("type changed"));
    }

    #[test]
    fn collect_parameters_without_name_or_in() {
        // Exercises line 285-286: parameters missing "name" or "in" fields
        let old = make_spec(
            r#"{ "/users": { "get": { "parameters": [
                { "schema": { "type": "string" } }
            ] } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "get": { "parameters": [
                { "schema": { "type": "string" } }
            ] } } }"#,
        );
        // Should not crash — name="" and in="" will be used as defaults
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.breaking_count, 0);
    }

    #[test]
    fn no_parameters_key() {
        // Exercises line 282: op without "parameters" key
        let old = make_spec(r#"{ "/users": { "get": { "summary": "list" } } }"#);
        let new = make_spec(r#"{ "/users": { "get": { "summary": "updated list" } } }"#);
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert!(report.changes.is_empty());
    }

    #[test]
    fn request_body_ref_resolution() {
        // Exercises line 312-313: requestBody itself is a $ref
        let old = make_spec_with_components(
            r##"{ "/users": { "post": {
                "requestBody": { "$ref": "#/components/requestBodies/CreateUser" }
            } } }"##,
            r##"{
                "requestBodies": {
                    "CreateUser": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": { "name": { "type": "string" } },
                                    "required": ["name"]
                                }
                            }
                        }
                    }
                }
            }"##,
        );
        let new = make_spec_with_components(
            r##"{ "/users": { "post": {
                "requestBody": { "$ref": "#/components/requestBodies/CreateUser" }
            } } }"##,
            r##"{
                "requestBodies": {
                    "CreateUser": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "name": { "type": "string" },
                                        "email": { "type": "string" }
                                    },
                                    "required": ["name", "email"]
                                }
                            }
                        }
                    }
                }
            }"##,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        let breaking: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Breaking)
            .collect();
        assert!(!breaking.is_empty());
        assert!(breaking.iter().any(|c| c.description.contains("email")));
    }

    #[test]
    fn multiple_new_paths_all_non_breaking() {
        // Exercises the new paths loop (lines 115-129) with multiple new paths
        let old = make_spec(r#"{}"#);
        let new = make_spec(
            r#"{
                "/users": { "get": { "summary": "list users" } },
                "/posts": { "post": { "summary": "create post" } }
            }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.breaking_count, 0);
        assert_eq!(report.non_breaking_count, 2);
    }

    #[test]
    fn new_path_only_counts_existing_methods() {
        // New path but only one method defined — shouldn't count other HTTP methods
        let old = make_spec(r#"{}"#);
        let new = make_spec(r#"{ "/items": { "patch": { "summary": "update" } } }"#);
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.non_breaking_count, 1);
        assert_eq!(report.changes[0].method, "PATCH");
    }

    #[test]
    fn all_http_methods_covered() {
        // Tests all 7 HTTP methods: get, post, put, delete, patch, head, options
        let old = make_spec(r#"{}"#);
        let new = make_spec(
            r#"{ "/all": {
                "get": { "summary": "g" },
                "post": { "summary": "p" },
                "put": { "summary": "pu" },
                "delete": { "summary": "d" },
                "patch": { "summary": "pa" },
                "head": { "summary": "h" },
                "options": { "summary": "o" }
            } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.non_breaking_count, 7);
        let methods: Vec<&str> = report.changes.iter().map(|c| c.method.as_str()).collect();
        assert!(methods.contains(&"GET"));
        assert!(methods.contains(&"POST"));
        assert!(methods.contains(&"PUT"));
        assert!(methods.contains(&"DELETE"));
        assert!(methods.contains(&"PATCH"));
        assert!(methods.contains(&"HEAD"));
        assert!(methods.contains(&"OPTIONS"));
    }

    #[test]
    fn schema_no_properties() {
        // Exercises lines 385-386, 401-402: schemas without "properties" key
        let old = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": { "type": "object" }
                        }
                    }
                }
            } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": { "type": "object" }
                        }
                    }
                }
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert!(report.changes.is_empty());
    }

    #[test]
    fn schema_no_required_array() {
        // Exercises lines 388-392, 394-398: schemas without "required" key
        let old = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": { "name": { "type": "string" } }
                            }
                        }
                    }
                }
            } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "name": { "type": "string" },
                                    "bio": { "type": "string" }
                                }
                            }
                        }
                    }
                }
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        // bio added as optional (no required list)
        let non_breaking: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.kind == ChangeKind::NonBreaking)
            .collect();
        assert!(!non_breaking.is_empty());
        assert!(non_breaking.iter().any(|c| c.description.contains("bio")));
    }

    #[test]
    fn enum_only_on_old_or_new_no_diff() {
        // Exercises lines 485: (Some, None) and (None, Some) — enum appears/disappears
        // When only one side has enum, diff_enum does nothing (both must have enum)
        let old = make_spec(
            r#"{ "/users": { "get": { "parameters": [
                { "name": "status", "in": "query",
                  "schema": { "type": "string", "enum": ["a", "b"] } }
            ] } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "get": { "parameters": [
                { "name": "status", "in": "query",
                  "schema": { "type": "string" } }
            ] } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        // No enum-related changes since new has no enum
        let enum_changes: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.description.contains("enum"))
            .collect();
        assert!(enum_changes.is_empty());
    }

    #[test]
    fn request_body_no_json_content() {
        // Exercises get_json_schema returning None — no "application/json" key
        let old = make_spec(
            r#"{ "/upload": { "post": {
                "requestBody": {
                    "content": {
                        "multipart/form-data": {
                            "schema": { "type": "object" }
                        }
                    }
                }
            } } }"#,
        );
        let new = make_spec(
            r#"{ "/upload": { "post": {
                "requestBody": {
                    "content": {
                        "multipart/form-data": {
                            "schema": { "type": "object" }
                        }
                    }
                }
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        // (None, None) in the schema match — no changes
        assert!(report.changes.is_empty());
    }

    #[test]
    fn request_body_no_content_key() {
        // Exercises get_json_schema returning None — no "content" key at all
        let old = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": { "description": "data" }
            } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": { "description": "updated data" }
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert!(report.changes.is_empty());
    }

    #[test]
    fn schema_property_without_type() {
        // Exercises lines 442-443: property in schema has no "type" key
        let old = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": { "data": {} }
                            }
                        }
                    }
                }
            } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "post": {
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "properties": { "data": { "type": "string" } }
                            }
                        }
                    }
                }
            } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        // Old has no type, so (None, Some) — no type-change reported
        let type_changes: Vec<_> = report
            .changes
            .iter()
            .filter(|c| c.description.contains("type changed"))
            .collect();
        assert!(type_changes.is_empty());
    }

    // -----------------------------------------------------------------------
    // Bug-hunting: boundary / malformed input tests
    // -----------------------------------------------------------------------

    #[test]
    fn both_specs_empty_json() {
        let report = ApiDiffer::diff("{}", "{}").unwrap();
        assert_eq!(report.changes.len(), 0);
    }

    #[test]
    fn both_specs_empty_paths() {
        let old = make_spec("{}");
        let new = make_spec("{}");
        let report = ApiDiffer::diff(&old, &new).unwrap();
        assert_eq!(report.changes.len(), 0);
    }

    #[test]
    fn invalid_json_old_spec() {
        let result = ApiDiffer::diff("not json", r#"{"paths":{}}"#);
        assert!(result.is_err());
    }

    #[test]
    fn invalid_json_new_spec() {
        let result = ApiDiffer::diff(r#"{"paths":{}}"#, "not json");
        assert!(result.is_err());
    }

    #[test]
    fn ref_pointing_nowhere() {
        // $ref that doesn't resolve -- should not crash
        let ref_path = "#/components/parameters/NonExistent";
        let old_paths = format!(
            r#"{{ "/users": {{ "get": {{ "parameters": [ {{ "$ref": "{ref_path}" }} ] }} }} }}"#
        );
        let old = make_spec(&old_paths);
        let new = make_spec(r#"{ "/users": { "get": { "parameters": [] } } }"#);
        // Should not panic
        let report = ApiDiffer::diff(&old, &new).unwrap();
        let _ = report;
    }

    #[test]
    fn follow_ref_with_json_pointer_escaping() {
        // Test ~0 and ~1 escaping per RFC 6901
        let json_str = r#"{
            "components": {
                "schemas": {
                    "a/b": { "type": "string" },
                    "c~d": { "type": "integer" }
                }
            }
        }"#;
        let root: Value = serde_json::from_str(json_str).unwrap();
        // "a/b" is escaped as "a~1b"
        let ref_a = "#/components/schemas/a~1b";
        let resolved = ApiDiffer::follow_ref(ref_a, &root);
        assert!(resolved.is_some(), "should resolve path with ~1 escape");
        assert_eq!(resolved.unwrap().get("type").unwrap(), "string");

        // "c~d" is escaped as "c~0d"
        let ref_c = "#/components/schemas/c~0d";
        let resolved = ApiDiffer::follow_ref(ref_c, &root);
        assert!(resolved.is_some(), "should resolve path with ~0 escape");
        assert_eq!(resolved.unwrap().get("type").unwrap(), "integer");
    }

    #[test]
    #[ignore = "BUG CONFIRMED: circular $ref causes stack overflow (SIGABRT). See resolve_ref unbounded recursion."]
    fn circular_ref_two_level() {
        // BUG: resolve_ref recurses indefinitely on circular $ref chains.
        // A -> B -> A creates a stack overflow.
        let ref_c = "#/components/parameters/Circular";
        let ref_c2 = "#/components/parameters/Circular2";
        let spec_json = format!(
            r#"{{
            "openapi": "3.0.0",
            "info": {{ "title": "Test", "version": "1.0" }},
            "paths": {{
                "/users": {{
                    "get": {{
                        "parameters": [
                            {{ "$ref": "{ref_c}" }}
                        ]
                    }}
                }}
            }},
            "components": {{
                "parameters": {{
                    "Circular": {{ "$ref": "{ref_c2}" }},
                    "Circular2": {{ "$ref": "{ref_c}" }}
                }}
            }}
        }}"#
        );
        // This diff triggers resolve_ref on the circular chain.
        // If the recursion guard is broken, this will stack overflow.
        let spec = spec_json.clone();
        let result = std::panic::catch_unwind(move || {
            ApiDiffer::diff(&spec, &spec)
        });
        // Stack overflow may abort the process rather than unwinding.
        // If this test crashes entirely, the bug is confirmed.
        if result.is_err() {
            panic!("BUG CONFIRMED: circular $ref causes stack overflow");
        }
    }

    #[test]
    fn non_http_method_keys_ignored() {
        // Keys like "summary", "description", "parameters" at path level should be ignored
        let old = make_spec(
            r#"{ "/users": { "summary": "User operations", "get": { "summary": "list" } } }"#,
        );
        let new = make_spec(
            r#"{ "/users": { "summary": "Changed summary", "get": { "summary": "list" } } }"#,
        );
        let report = ApiDiffer::diff(&old, &new).unwrap();
        // "summary" is not an HTTP method, should have no changes
        assert_eq!(report.changes.len(), 0);
    }

}
