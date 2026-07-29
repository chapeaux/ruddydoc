//! MCP (Model Context Protocol) server for RuddyDoc.
//!
//! Implements the MCP stdio transport: reads JSON-RPC 2.0 messages from
//! stdin (newline-delimited) and writes responses to stdout. This lets
//! AI agents like Claude Code use RuddyDoc as a tool server.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::state::ServerState;

// ---------------------------------------------------------------------------
// JSON-RPC types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

impl JsonRpcResponse {
    fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// MCP tool schemas
// ---------------------------------------------------------------------------

/// Return the MCP tool schemas as JSON values.
fn tool_schemas() -> Vec<Value> {
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
                        "description": "Output format (default: json). One of: json, markdown, html, text, turtle, ntriples"
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
                        "description": "Filter by element type (e.g., 'Paragraph', 'SectionHeader')"
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
            "name": "introspect_document",
            "description": "Summarize a document's RDF schema: triple/entity counts, \
                classes, predicates, characteristic sets, join hints, and vocabularies. \
                Mined directly from the store's indexes -- use this before writing SPARQL \
                against a document you haven't queried before.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "document_id": {
                        "type": "string",
                        "description": "Document ID returned from convert_document"
                    }
                },
                "required": ["document_id"]
            }
        }),
        serde_json::json!({
            "name": "list_classes",
            "description": "List the RDF classes (types) present in a document, with \
                instance counts, by descending count.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "document_id": {
                        "type": "string",
                        "description": "Document ID returned from convert_document"
                    }
                },
                "required": ["document_id"]
            }
        }),
        serde_json::json!({
            "name": "list_prefixes",
            "description": "List the namespaces/prefixes in use in a document, with \
                term counts, by descending count.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "document_id": {
                        "type": "string",
                        "description": "Document ID returned from convert_document"
                    }
                },
                "required": ["document_id"]
            }
        }),
        serde_json::json!({
            "name": "search_text",
            "description": "Full-text (BM25) search over a document's string literals -- \
                finds matching text without needing to write SPARQL. Returns each match's \
                text, relevance score, and the (subject, predicate) pairs it appears on.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "document_id": {
                        "type": "string",
                        "description": "Document ID returned from convert_document"
                    },
                    "query": {
                        "type": "string",
                        "description": "Search terms"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of matches to return (default: 10)"
                    }
                },
                "required": ["document_id", "query"]
            }
        }),
        serde_json::json!({
            "name": "embed_document",
            "description": "Chunk a document and embed each chunk via the configured \
                embedding provider (set RUDDYDOC_EMBEDDING_URL), for later semantic_search. \
                Errors if no provider is configured.",
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
                    }
                },
                "required": ["document_id"]
            }
        }),
        serde_json::json!({
            "name": "semantic_search",
            "description": "Semantic (embedding-similarity) search over a document's chunks \
                -- finds conceptually related text even without exact keyword matches. \
                Requires embed_document to have been run first, and an embedding provider \
                to be configured.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "document_id": {
                        "type": "string",
                        "description": "Document ID returned from convert_document"
                    },
                    "query": {
                        "type": "string",
                        "description": "Natural-language search query"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of matches to return (default: 5)"
                    }
                },
                "required": ["document_id", "query"]
            }
        }),
        serde_json::json!({
            "name": "ask_document",
            "description": "Ask a natural-language question about a document -- no SPARQL \
                required. Grounds the question against the document's schema, generates a \
                query with the configured LLM (set RUDDYDOC_LLM_URL), validates/repairs it, \
                and executes it. Errors if no LLM provider is configured.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "document_id": {
                        "type": "string",
                        "description": "Document ID returned from convert_document"
                    },
                    "question": {
                        "type": "string",
                        "description": "Natural-language question about the document"
                    }
                },
                "required": ["document_id", "question"]
            }
        }),
        serde_json::json!({
            "name": "validate_document",
            "description": "Re-validate a document against the ontology's SHACL shapes on \
                demand. Returns a report ({conforms, results}); unlike the validation \
                attached to convert_document's response (a snapshot from initial \
                conversion), this reflects the document graph's current state.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "document_id": {
                        "type": "string",
                        "description": "Document ID returned from convert_document"
                    }
                },
                "required": ["document_id"]
            }
        }),
        serde_json::json!({
            "name": "canonicalize_document",
            "description": "Compute an RDFC-1.0 canonical-graph hash (hex SHA-256) for a \
                document's current graph state. Useful for detecting when different source \
                formats (or re-conversions) produce semantically-identical graphs, since it \
                hashes the derived RDF content rather than the raw input bytes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "document_id": {
                        "type": "string",
                        "description": "Document ID returned from convert_document"
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

// ---------------------------------------------------------------------------
// MCP stdio server
// ---------------------------------------------------------------------------

/// Run the MCP server on stdio (stdin/stdout, newline-delimited JSON-RPC).
pub async fn run_stdio() -> ruddydoc_core::Result<()> {
    let state = Arc::new(ServerState::new()?);

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let reader = tokio::io::BufReader::new(stdin);
    let mut writer = stdout;

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse::error(
                    Value::Null,
                    -32700,
                    format!("parse error: {e}"),
                );
                let out = serde_json::to_string(&resp).unwrap_or_default();
                writer.write_all(out.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
                continue;
            }
        };

        // Notifications have no id and expect no response
        if request.id.is_none() {
            continue;
        }

        let id = request.id.unwrap_or(Value::Null);
        let response = handle_request(&state, &request.method, request.params.as_ref()).await;
        let resp = match response {
            Ok(result) => JsonRpcResponse::success(id, result),
            Err(msg) => JsonRpcResponse::error(id, -32603, msg),
        };

        let out = serde_json::to_string(&resp).unwrap_or_default();
        writer.write_all(out.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
    }

    Ok(())
}

/// Dispatch a JSON-RPC method to the appropriate handler.
async fn handle_request(
    state: &Arc<ServerState>,
    method: &str,
    params: Option<&Value>,
) -> Result<Value, String> {
    match method {
        "initialize" => handle_initialize(params),
        "tools/list" => handle_tools_list(),
        "tools/call" => handle_tools_call(state, params).await,
        "ping" => Ok(serde_json::json!({})),
        _ => Err(format!("unknown method: {method}")),
    }
}

fn handle_initialize(_params: Option<&Value>) -> Result<Value, String> {
    Ok(serde_json::json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "ruddydoc",
            "version": env!("CARGO_PKG_VERSION")
        }
    }))
}

fn handle_tools_list() -> Result<Value, String> {
    Ok(serde_json::json!({
        "tools": tool_schemas()
    }))
}

async fn handle_tools_call(
    state: &Arc<ServerState>,
    params: Option<&Value>,
) -> Result<Value, String> {
    let params = params.ok_or("missing params")?;
    let tool_name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or("missing tool name")?;
    let arguments = params.get("arguments").cloned().unwrap_or(serde_json::json!({}));

    let result = match tool_name {
        "convert_document" => tool_convert_document(state, &arguments).await,
        "query_document" => tool_query_document(state, &arguments).await,
        "export_document" => tool_export_document(state, &arguments).await,
        "list_elements" => tool_list_elements(state, &arguments).await,
        "chunk_document" => tool_chunk_document(state, &arguments).await,
        "introspect_document" => tool_introspect_document(state, &arguments).await,
        "list_classes" => tool_list_classes(state, &arguments).await,
        "list_prefixes" => tool_list_prefixes(state, &arguments).await,
        "validate_document" => tool_validate_document(state, &arguments).await,
        "canonicalize_document" => tool_canonicalize_document(state, &arguments).await,
        "search_text" => tool_search_text(state, &arguments).await,
        "embed_document" => tool_embed_document(state, &arguments).await,
        "semantic_search" => tool_semantic_search(state, &arguments).await,
        "ask_document" => tool_ask_document(state, &arguments).await,
        "list_documents" => tool_list_documents(state).await,
        "list_formats" => tool_list_formats(),
        _ => Err(format!("unknown tool: {tool_name}")),
    };

    match result {
        Ok(content) => {
            let text = if content.is_string() {
                content.as_str().unwrap_or("").to_string()
            } else {
                serde_json::to_string_pretty(&content).unwrap_or_default()
            };
            Ok(serde_json::json!({
                "content": [{ "type": "text", "text": text }]
            }))
        }
        Err(e) => Ok(serde_json::json!({
            "content": [{ "type": "text", "text": e }],
            "isError": true
        })),
    }
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

async fn tool_convert_document(
    state: &Arc<ServerState>,
    args: &Value,
) -> Result<Value, String> {
    let source = args
        .get("source")
        .and_then(|v| v.as_str())
        .ok_or("missing 'source' parameter")?;

    let record = state
        .convert_file(source)
        .await
        .map_err(|e| format!("conversion failed: {e}"))?;

    Ok(serde_json::json!({
        "id": record.id,
        "format": record.meta.format.to_string(),
        "file_size": record.meta.file_size,
        "page_count": record.meta.page_count,
        "graph_iri": record.graph_iri,
        "validation": record.validation,
    }))
}

async fn tool_query_document(
    state: &Arc<ServerState>,
    args: &Value,
) -> Result<Value, String> {
    let doc_id = args
        .get("document_id")
        .and_then(|v| v.as_str())
        .ok_or("missing 'document_id' parameter")?;
    let sparql = args
        .get("sparql")
        .and_then(|v| v.as_str())
        .ok_or("missing 'sparql' parameter")?;

    let results = state
        .query_document(doc_id, sparql)
        .await
        .map_err(|e| format!("query failed: {e}"))?;

    Ok(results)
}

async fn tool_export_document(
    state: &Arc<ServerState>,
    args: &Value,
) -> Result<Value, String> {
    let doc_id = args
        .get("document_id")
        .and_then(|v| v.as_str())
        .ok_or("missing 'document_id' parameter")?;
    let format = args
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("json");

    let exported = state
        .export_document(doc_id, format)
        .await
        .map_err(|e| format!("export failed: {e}"))?;

    Ok(Value::String(exported))
}

async fn tool_list_elements(
    state: &Arc<ServerState>,
    args: &Value,
) -> Result<Value, String> {
    let doc_id = args
        .get("document_id")
        .and_then(|v| v.as_str())
        .ok_or("missing 'document_id' parameter")?;
    let element_type = args.get("element_type").and_then(|v| v.as_str());

    let results = state
        .list_elements(doc_id, element_type)
        .await
        .map_err(|e| format!("list elements failed: {e}"))?;

    Ok(results)
}

async fn tool_introspect_document(
    state: &Arc<ServerState>,
    args: &Value,
) -> Result<Value, String> {
    let doc_id = args
        .get("document_id")
        .and_then(|v| v.as_str())
        .ok_or("missing 'document_id' parameter")?;

    state
        .introspect_document(doc_id)
        .await
        .map_err(|e| format!("introspection failed: {e}"))
}

async fn tool_list_classes(state: &Arc<ServerState>, args: &Value) -> Result<Value, String> {
    let doc_id = args
        .get("document_id")
        .and_then(|v| v.as_str())
        .ok_or("missing 'document_id' parameter")?;

    state
        .list_classes(doc_id)
        .await
        .map_err(|e| format!("list classes failed: {e}"))
}

async fn tool_list_prefixes(state: &Arc<ServerState>, args: &Value) -> Result<Value, String> {
    let doc_id = args
        .get("document_id")
        .and_then(|v| v.as_str())
        .ok_or("missing 'document_id' parameter")?;

    state
        .list_prefixes(doc_id)
        .await
        .map_err(|e| format!("list prefixes failed: {e}"))
}

async fn tool_search_text(state: &Arc<ServerState>, args: &Value) -> Result<Value, String> {
    let doc_id = args
        .get("document_id")
        .and_then(|v| v.as_str())
        .ok_or("missing 'document_id' parameter")?;
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or("missing 'query' parameter")?;
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

    state
        .search_text(doc_id, query, limit)
        .await
        .map_err(|e| format!("search failed: {e}"))
}

async fn tool_embed_document(state: &Arc<ServerState>, args: &Value) -> Result<Value, String> {
    let doc_id = args
        .get("document_id")
        .and_then(|v| v.as_str())
        .ok_or("missing 'document_id' parameter")?;
    let max_tokens = args
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(512) as usize;

    let count = state
        .embed_document(doc_id, max_tokens)
        .await
        .map_err(|e| format!("embedding failed: {e}"))?;

    Ok(serde_json::json!({ "chunks_embedded": count }))
}

async fn tool_semantic_search(state: &Arc<ServerState>, args: &Value) -> Result<Value, String> {
    let doc_id = args
        .get("document_id")
        .and_then(|v| v.as_str())
        .ok_or("missing 'document_id' parameter")?;
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or("missing 'query' parameter")?;
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;

    state
        .semantic_search(doc_id, query, limit)
        .await
        .map_err(|e| format!("semantic search failed: {e}"))
}

async fn tool_ask_document(state: &Arc<ServerState>, args: &Value) -> Result<Value, String> {
    let doc_id = args
        .get("document_id")
        .and_then(|v| v.as_str())
        .ok_or("missing 'document_id' parameter")?;
    let question = args
        .get("question")
        .and_then(|v| v.as_str())
        .ok_or("missing 'question' parameter")?;

    state
        .ask_document(doc_id, question)
        .await
        .map_err(|e| format!("ask_document failed: {e}"))
}

async fn tool_validate_document(state: &Arc<ServerState>, args: &Value) -> Result<Value, String> {
    let doc_id = args
        .get("document_id")
        .and_then(|v| v.as_str())
        .ok_or("missing 'document_id' parameter")?;

    state
        .validate_document(doc_id)
        .await
        .map_err(|e| format!("validation failed: {e}"))
}

async fn tool_canonicalize_document(
    state: &Arc<ServerState>,
    args: &Value,
) -> Result<Value, String> {
    let doc_id = args
        .get("document_id")
        .and_then(|v| v.as_str())
        .ok_or("missing 'document_id' parameter")?;

    let hash = state
        .canonicalize_document(doc_id)
        .await
        .map_err(|e| format!("canonicalization failed: {e}"))?;

    Ok(serde_json::json!({ "canonical_hash": hash }))
}

async fn tool_chunk_document(
    state: &Arc<ServerState>,
    args: &Value,
) -> Result<Value, String> {
    let doc_id = args
        .get("document_id")
        .and_then(|v| v.as_str())
        .ok_or("missing 'document_id' parameter")?;
    let max_tokens = args
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(512) as usize;

    let chunks = state
        .chunk_document(doc_id, max_tokens)
        .await
        .map_err(|e| format!("chunking failed: {e}"))?;

    Ok(serde_json::json!({
        "chunks": chunks,
        "count": chunks.len(),
        "max_tokens": max_tokens,
    }))
}

async fn tool_list_documents(state: &Arc<ServerState>) -> Result<Value, String> {
    let docs = state.documents.read().await;
    let documents: Vec<Value> = docs
        .values()
        .map(|record| {
            serde_json::json!({
                "id": record.id,
                "format": record.meta.format.to_string(),
                "file_size": record.meta.file_size,
                "graph_iri": record.graph_iri,
            })
        })
        .collect();

    Ok(serde_json::json!({ "documents": documents }))
}

fn tool_list_formats() -> Result<Value, String> {
    let input_formats: Vec<Value> = ruddydoc_converter::list_supported_formats()
        .iter()
        .map(|f| {
            serde_json::json!({
                "format": f.format.to_string(),
                "extensions": f.extensions,
                "mime_type": f.mime_type,
            })
        })
        .collect();

    let output_formats = vec!["json", "markdown", "html", "text", "turtle", "ntriples"];

    Ok(serde_json::json!({
        "input_formats": input_formats,
        "output_formats": output_formats,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_schemas_are_valid_json() {
        let schemas = tool_schemas();
        assert!(!schemas.is_empty());

        for schema in &schemas {
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
        let schemas = tool_schemas();
        let names: Vec<&str> = schemas
            .iter()
            .filter_map(|s| s.get("name").and_then(|v| v.as_str()))
            .collect();

        assert!(names.contains(&"convert_document"));
        assert!(names.contains(&"query_document"));
        assert!(names.contains(&"export_document"));
        assert!(names.contains(&"list_elements"));
        assert!(names.contains(&"chunk_document"));
        assert!(names.contains(&"introspect_document"));
        assert!(names.contains(&"list_classes"));
        assert!(names.contains(&"list_prefixes"));
        assert!(names.contains(&"validate_document"));
        assert!(names.contains(&"canonicalize_document"));
        assert!(names.contains(&"search_text"));
        assert!(names.contains(&"embed_document"));
        assert!(names.contains(&"semantic_search"));
        assert!(names.contains(&"ask_document"));
        assert!(names.contains(&"list_documents"));
        assert!(names.contains(&"list_formats"));
    }

    #[test]
    fn convert_document_requires_source() {
        let schemas = tool_schemas();
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

    #[test]
    fn initialize_response_has_required_fields() {
        let result = handle_initialize(None).unwrap();
        assert!(result.get("protocolVersion").is_some());
        assert!(result.get("capabilities").is_some());
        assert!(result.get("serverInfo").is_some());
    }

    #[test]
    fn tools_list_returns_tools() {
        let result = handle_tools_list().unwrap();
        let tools = result.get("tools").and_then(|v| v.as_array()).unwrap();
        assert_eq!(tools.len(), 16);
    }
}
