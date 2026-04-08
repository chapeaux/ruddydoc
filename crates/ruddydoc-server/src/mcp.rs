//! MCP tool definitions for RuddyDoc.
//!
//! These define the MCP tool schemas for AI agent integration. The actual
//! MCP protocol wiring (via `rust-mcp-sdk`) will be added in a future
//! iteration. For now, this module provides the tool schema definitions
//! that can be served via the REST API at `/mcp/tools` or used by an
//! MCP transport layer.

/// MCP tool definitions for the RuddyDoc document server.
pub struct McpToolDefinitions;

impl McpToolDefinitions {
    /// Return the MCP tool schemas as JSON values.
    ///
    /// Each tool schema follows the MCP specification with `name`,
    /// `description`, and `inputSchema` fields.
    pub fn tool_schemas() -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({
                "name": "convert_document",
                "description": "Convert a document file to RuddyDoc's knowledge graph. \
                    Accepts a file path, detects the format automatically, parses it \
                    into an RDF graph, and returns a document ID for further operations.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "source": {
                            "type": "string",
                            "description": "File path to the document to convert"
                        },
                        "format": {
                            "type": "string",
                            "description": "Force input format (optional, auto-detected if omitted)"
                        }
                    },
                    "required": ["source"]
                }
            }),
            serde_json::json!({
                "name": "query_document",
                "description": "Run a SPARQL query against a converted document's knowledge graph. \
                    The query is automatically scoped to the document's named graph.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "document_id": {
                            "type": "string",
                            "description": "Document ID returned from convert_document"
                        },
                        "sparql": {
                            "type": "string",
                            "description": "SPARQL SELECT or ASK query"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results (default: 100)"
                        },
                        "offset": {
                            "type": "integer",
                            "description": "Number of results to skip (default: 0)"
                        }
                    },
                    "required": ["document_id", "sparql"]
                }
            }),
            serde_json::json!({
                "name": "export_document",
                "description": "Export a converted document in a specified format. \
                    Supported formats: json, markdown, html, text, turtle, ntriples.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "document_id": {
                            "type": "string",
                            "description": "Document ID returned from convert_document"
                        },
                        "format": {
                            "type": "string",
                            "description": "Output format (default: json)"
                        }
                    },
                    "required": ["document_id"]
                }
            }),
            serde_json::json!({
                "name": "list_elements",
                "description": "List structural elements in a converted document, \
                    optionally filtered by type (e.g., Paragraph, SectionHeader, Code, \
                    TableElement, ListItem, PictureElement).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "document_id": {
                            "type": "string",
                            "description": "Document ID returned from convert_document"
                        },
                        "element_type": {
                            "type": "string",
                            "description": "Filter by element type (e.g., 'Paragraph')"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results (default: 50)"
                        },
                        "offset": {
                            "type": "integer",
                            "description": "Number of results to skip (default: 0)"
                        }
                    },
                    "required": ["document_id"]
                }
            }),
            serde_json::json!({
                "name": "chunk_document",
                "description": "Chunk a converted document for RAG (Retrieval Augmented \
                    Generation). Uses hierarchical chunking that respects document \
                    structure and heading boundaries.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "document_id": {
                            "type": "string",
                            "description": "Document ID returned from convert_document"
                        },
                        "max_tokens": {
                            "type": "integer",
                            "description": "Maximum tokens per chunk (default: 512)"
                        },
                        "include_headings": {
                            "type": "boolean",
                            "description": "Prepend heading hierarchy to chunk text (default: true)"
                        }
                    },
                    "required": ["document_id"]
                }
            }),
            serde_json::json!({
                "name": "list_documents",
                "description": "List all documents that have been converted in this server session.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            }),
            serde_json::json!({
                "name": "list_formats",
                "description": "List all supported input and output formats.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            }),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_schemas_are_valid_json() {
        let schemas = McpToolDefinitions::tool_schemas();
        assert!(!schemas.is_empty());

        for schema in &schemas {
            // Each tool must have name, description, and inputSchema
            assert!(
                schema.get("name").is_some(),
                "tool missing 'name': {schema}"
            );
            assert!(
                schema.get("description").is_some(),
                "tool missing 'description': {schema}"
            );
            assert!(
                schema.get("inputSchema").is_some(),
                "tool missing 'inputSchema': {schema}"
            );

            // inputSchema must have type: "object"
            let input_schema = schema.get("inputSchema").unwrap();
            assert_eq!(
                input_schema.get("type").and_then(|v| v.as_str()),
                Some("object"),
                "inputSchema type must be 'object'"
            );
        }
    }

    #[test]
    fn tool_schemas_have_expected_tools() {
        let schemas = McpToolDefinitions::tool_schemas();
        let names: Vec<&str> = schemas
            .iter()
            .filter_map(|s| s.get("name").and_then(|v| v.as_str()))
            .collect();

        assert!(names.contains(&"convert_document"));
        assert!(names.contains(&"query_document"));
        assert!(names.contains(&"export_document"));
        assert!(names.contains(&"list_elements"));
        assert!(names.contains(&"chunk_document"));
        assert!(names.contains(&"list_documents"));
        assert!(names.contains(&"list_formats"));
    }

    #[test]
    fn convert_document_requires_source() {
        let schemas = McpToolDefinitions::tool_schemas();
        let convert = schemas
            .iter()
            .find(|s| s.get("name").and_then(|v| v.as_str()) == Some("convert_document"))
            .expect("convert_document tool should exist");

        let required = convert
            .get("inputSchema")
            .and_then(|s| s.get("required"))
            .and_then(|r| r.as_array())
            .expect("should have required array");

        let required_names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();

        assert!(required_names.contains(&"source"));
    }
}
