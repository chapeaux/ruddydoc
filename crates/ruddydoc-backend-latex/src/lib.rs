//! LaTeX parser backend for RuddyDoc.
//!
//! Custom recursive-descent parser for LaTeX documents. Tokenizes the input
//! into commands, braces, environment markers, and text; then walks the
//! token stream emitting RDF triples based on recognized commands and
//! environments. No external parser dependencies are used.

use sha2::{Digest, Sha256};

use ruddydoc_core::{
    DocumentBackend, DocumentHash, DocumentMeta, DocumentSource, DocumentStore, InputFormat,
};
use ruddydoc_ontology as ont;

// ---------------------------------------------------------------------------
// SHA-256 hashing (same pattern as backend-md)
// ---------------------------------------------------------------------------

/// Compute a SHA-256 hash of content bytes, returning a hex string.
fn compute_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex::encode(result)
}

/// Hex-encode bytes without pulling in the `hex` crate.
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes.as_ref().iter().fold(String::new(), |mut s, b| {
            use std::fmt::Write;
            let _ = write!(s, "{b:02x}");
            s
        })
    }
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

/// A single LaTeX token produced by the tokenizer.
#[derive(Debug, Clone, PartialEq)]
enum Token {
    /// A LaTeX command like `\section`, `\textbf`, etc.
    Command(String),
    /// Opening brace `{`.
    OpenBrace,
    /// Closing brace `}`.
    CloseBrace,
    /// Opening bracket `[`.
    OpenBracket,
    /// Closing bracket `]`.
    CloseBracket,
    /// Plain text between commands.
    Text(String),
    /// `\begin{envname}`
    BeginEnv(String),
    /// `\end{envname}`
    EndEnv(String),
    /// Display math `$$...$$`
    DisplayMath(String),
    /// Bracket display math `\[...\]`
    BracketMath(String),
}

/// Tokenize a LaTeX source string into a sequence of tokens.
///
/// Handles:
/// - `%` line comments (stripped)
/// - `\begin{env}` and `\end{env}` as single tokens
/// - `\command` tokens
/// - `{`, `}`, `[`, `]` brace tokens
/// - `$$...$$` display math
/// - `\[...\]` bracket math
/// - Remaining text
fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut text_buf = String::new();

    while i < len {
        let c = chars[i];

        // --- Line comments ---
        if c == '%' {
            // Skip to end of line
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // --- Display math: $$...$$ ---
        if c == '$' && i + 1 < len && chars[i + 1] == '$' {
            flush_text(&mut text_buf, &mut tokens);
            i += 2; // skip opening $$
            let mut math = String::new();
            while i + 1 < len && !(chars[i] == '$' && chars[i + 1] == '$') {
                math.push(chars[i]);
                i += 1;
            }
            if i + 1 < len {
                i += 2; // skip closing $$
            }
            tokens.push(Token::DisplayMath(math.trim().to_string()));
            continue;
        }

        // --- Backslash sequences ---
        if c == '\\' {
            // Bracket math: \[...\]
            if i + 1 < len && chars[i + 1] == '[' {
                flush_text(&mut text_buf, &mut tokens);
                i += 2; // skip \[
                let mut math = String::new();
                while i + 1 < len && !(chars[i] == '\\' && chars[i + 1] == ']') {
                    math.push(chars[i]);
                    i += 1;
                }
                if i + 1 < len {
                    i += 2; // skip \]
                }
                tokens.push(Token::BracketMath(math.trim().to_string()));
                continue;
            }

            // Read the command name
            let mut cmd = String::new();
            i += 1; // skip the backslash
            while i < len && chars[i].is_ascii_alphabetic() {
                cmd.push(chars[i]);
                i += 1;
            }

            if cmd.is_empty() {
                // Escaped character like \\ or \% or \{
                if i < len {
                    if chars[i] == '\\' {
                        // `\\` is a line-break command in LaTeX. Emit as a
                        // command so that collect_raw_text can reconstruct it.
                        flush_text(&mut text_buf, &mut tokens);
                        tokens.push(Token::Command("\\".to_string()));
                        i += 1;
                    } else {
                        text_buf.push(chars[i]);
                        i += 1;
                    }
                }
                continue;
            }

            // Check for \begin{env} or \end{env}
            if cmd == "begin" || cmd == "end" {
                // Consume optional whitespace before `{`
                while i < len && chars[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i < len && chars[i] == '{' {
                    i += 1; // skip {
                    let mut env_name = String::new();
                    while i < len && chars[i] != '}' {
                        env_name.push(chars[i]);
                        i += 1;
                    }
                    if i < len {
                        i += 1; // skip }
                    }
                    flush_text(&mut text_buf, &mut tokens);
                    if cmd == "begin" {
                        tokens.push(Token::BeginEnv(env_name));
                    } else {
                        tokens.push(Token::EndEnv(env_name));
                    }
                    continue;
                }
            }

            flush_text(&mut text_buf, &mut tokens);
            tokens.push(Token::Command(cmd));
            continue;
        }

        // --- Braces and brackets ---
        if c == '{' {
            flush_text(&mut text_buf, &mut tokens);
            tokens.push(Token::OpenBrace);
            i += 1;
            continue;
        }
        if c == '}' {
            flush_text(&mut text_buf, &mut tokens);
            tokens.push(Token::CloseBrace);
            i += 1;
            continue;
        }
        if c == '[' {
            flush_text(&mut text_buf, &mut tokens);
            tokens.push(Token::OpenBracket);
            i += 1;
            continue;
        }
        if c == ']' {
            flush_text(&mut text_buf, &mut tokens);
            tokens.push(Token::CloseBracket);
            i += 1;
            continue;
        }

        // --- Plain text ---
        text_buf.push(c);
        i += 1;
    }

    flush_text(&mut text_buf, &mut tokens);
    tokens
}

/// Flush accumulated text into the token stream if non-empty.
fn flush_text(buf: &mut String, tokens: &mut Vec<Token>) {
    if !buf.is_empty() {
        tokens.push(Token::Text(std::mem::take(buf)));
    }
}

// ---------------------------------------------------------------------------
// Parser context (mirrors backend-md's ParseContext)
// ---------------------------------------------------------------------------

/// State machine context for LaTeX parsing.
struct ParseContext<'a> {
    store: &'a dyn DocumentStore,
    doc_graph: &'a str,
    doc_hash: &'a str,
    /// Sequential reading order counter.
    reading_order: usize,
    /// Stack of parent element IRIs for tree structure.
    parent_stack: Vec<String>,
    /// The last element IRI at each depth, for next/previous linking.
    last_sibling_at_depth: Vec<Option<String>>,
    /// All element IRIs in order.
    all_elements: Vec<String>,
    /// Map from label names to element IRIs.
    label_map: Vec<(String, String)>,
    /// Pending cross-reference targets (label names referenced via \ref or \cite).
    pending_refs: Vec<(String, String)>,
    /// The IRI of the most recently emitted element (for attaching \label).
    last_element_iri: Option<String>,
}

impl<'a> ParseContext<'a> {
    fn new(store: &'a dyn DocumentStore, doc_graph: &'a str, doc_hash: &'a str) -> Self {
        Self {
            store,
            doc_graph,
            doc_hash,
            reading_order: 0,
            parent_stack: Vec::new(),
            last_sibling_at_depth: Vec::new(),
            all_elements: Vec::new(),
            label_map: Vec::new(),
            pending_refs: Vec::new(),
            last_element_iri: None,
        }
    }

    /// Generate a unique element IRI.
    fn element_iri(&self, kind: &str) -> String {
        ruddydoc_core::element_iri(self.doc_hash, &format!("{kind}-{}", self.reading_order))
    }

    /// Insert an element into the graph with its type, reading order, and tree links.
    fn emit_element(&mut self, element_iri: &str, class_name: &str) -> ruddydoc_core::Result<()> {
        let rdf_type = ont::rdf_iri("type");
        let class_iri = ont::iri(class_name);
        let doc_iri = ruddydoc_core::doc_iri(self.doc_hash);
        let g = self.doc_graph;

        // rdf:type
        self.store
            .insert_triple_into(element_iri, &rdf_type, &class_iri, g)?;

        // rdoc:readingOrder
        self.store.insert_literal(
            element_iri,
            &ont::iri(ont::PROP_READING_ORDER),
            &self.reading_order.to_string(),
            "integer",
            g,
        )?;

        // rdoc:hasElement (document -> element)
        self.store.insert_triple_into(
            &doc_iri,
            &ont::iri(ont::PROP_HAS_ELEMENT),
            element_iri,
            g,
        )?;

        // Parent-child links
        if let Some(parent) = self.parent_stack.last() {
            self.store.insert_triple_into(
                element_iri,
                &ont::iri(ont::PROP_PARENT_ELEMENT),
                parent,
                g,
            )?;
            self.store.insert_triple_into(
                parent,
                &ont::iri(ont::PROP_CHILD_ELEMENT),
                element_iri,
                g,
            )?;
        }

        // Previous/next sibling links
        let depth = self.parent_stack.len();
        while self.last_sibling_at_depth.len() <= depth {
            self.last_sibling_at_depth.push(None);
        }
        if let Some(prev) = &self.last_sibling_at_depth[depth] {
            self.store.insert_triple_into(
                prev,
                &ont::iri(ont::PROP_NEXT_ELEMENT),
                element_iri,
                g,
            )?;
            self.store.insert_triple_into(
                element_iri,
                &ont::iri(ont::PROP_PREVIOUS_ELEMENT),
                prev,
                g,
            )?;
        }
        self.last_sibling_at_depth[depth] = Some(element_iri.to_string());

        self.all_elements.push(element_iri.to_string());
        self.last_element_iri = Some(element_iri.to_string());
        self.reading_order += 1;

        Ok(())
    }

    /// Set text content on an element.
    fn set_text_content(&self, element_iri: &str, text: &str) -> ruddydoc_core::Result<()> {
        self.store.insert_literal(
            element_iri,
            &ont::iri(ont::PROP_TEXT_CONTENT),
            text,
            "string",
            self.doc_graph,
        )
    }

    /// Resolve pending cross-references after parsing is complete.
    fn resolve_refs(&self) -> ruddydoc_core::Result<()> {
        for (ref_iri, label_name) in &self.pending_refs {
            for (lbl, target_iri) in &self.label_map {
                if lbl == label_name {
                    self.store.insert_triple_into(
                        ref_iri,
                        &ont::iri(ont::PROP_REFERS_TO),
                        target_iri,
                        self.doc_graph,
                    )?;
                    break;
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Brace-group reader utility
// ---------------------------------------------------------------------------

/// Read the contents of a brace-delimited group `{...}` starting at `pos`.
/// `pos` should point to the `OpenBrace` token. Returns the collected text
/// and the index past the closing brace.
///
/// Handles nested braces by tracking depth.
fn read_brace_group(tokens: &[Token], pos: usize) -> (String, usize) {
    let mut i = pos;
    if i >= tokens.len() {
        return (String::new(), i);
    }
    // Skip the opening brace
    if tokens[i] == Token::OpenBrace {
        i += 1;
    }
    let mut depth = 1;
    let mut text = String::new();
    while i < tokens.len() && depth > 0 {
        match &tokens[i] {
            Token::OpenBrace => {
                depth += 1;
                if depth > 1 {
                    text.push('{');
                }
            }
            Token::CloseBrace => {
                depth -= 1;
                if depth > 0 {
                    text.push('}');
                }
            }
            Token::Text(t) => text.push_str(t),
            Token::Command(cmd) => {
                // Strip inline formatting commands; pass through their content
                match cmd.as_str() {
                    "textbf" | "textit" | "emph" | "texttt" | "textsf" | "textrm" | "textsc"
                    | "underline" => {
                        // These take a brace argument; we'll just continue
                        // and the content inside the braces will be accumulated.
                    }
                    "textasciitilde" => text.push('~'),
                    _ => {
                        // Unknown commands: skip (their arguments will be read naturally)
                    }
                }
            }
            Token::BeginEnv(_) | Token::EndEnv(_) => {
                // Should not appear inside a brace group normally
            }
            Token::DisplayMath(m) => {
                text.push_str(m);
            }
            Token::BracketMath(m) => {
                text.push_str(m);
            }
            Token::OpenBracket => text.push('['),
            Token::CloseBracket => text.push(']'),
        }
        i += 1;
    }
    (text.trim().to_string(), i)
}

/// Read an optional bracket argument `[...]` starting at `pos`.
/// Returns `Some(content)` and index past the bracket, or `None` with
/// the same index if there is no bracket group.
fn read_bracket_group(tokens: &[Token], pos: usize) -> (Option<String>, usize) {
    if pos >= tokens.len() {
        return (None, pos);
    }
    if tokens[pos] != Token::OpenBracket {
        return (None, pos);
    }
    let mut i = pos + 1;
    let mut text = String::new();
    while i < tokens.len() {
        if tokens[i] == Token::CloseBracket {
            i += 1;
            return (Some(text.trim().to_string()), i);
        }
        match &tokens[i] {
            Token::Text(t) => text.push_str(t),
            Token::Command(c) => {
                text.push('\\');
                text.push_str(c);
            }
            _ => {}
        }
        i += 1;
    }
    (Some(text.trim().to_string()), i)
}

// ---------------------------------------------------------------------------
// Environment body reader
// ---------------------------------------------------------------------------

/// Read all tokens from `pos` until `\end{env_name}` is found.
/// Returns the slice of tokens inside the environment and the index
/// past the EndEnv token.
fn find_env_end(tokens: &[Token], pos: usize, env_name: &str) -> (usize, usize) {
    let mut i = pos;
    let mut depth = 1;
    while i < tokens.len() {
        match &tokens[i] {
            Token::BeginEnv(name) if name == env_name => {
                depth += 1;
            }
            Token::EndEnv(name) if name == env_name => {
                depth -= 1;
                if depth == 0 {
                    return (i, i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    (i, i)
}

/// Collect raw text from a range of tokens (for verbatim/lstlisting/equation).
fn collect_raw_text(tokens: &[Token], start: usize, end: usize) -> String {
    let mut s = String::new();
    for token in &tokens[start..end] {
        match token {
            Token::Text(t) => s.push_str(t),
            Token::Command(c) => {
                s.push('\\');
                s.push_str(c);
            }
            Token::OpenBrace => s.push('{'),
            Token::CloseBrace => s.push('}'),
            Token::OpenBracket => s.push('['),
            Token::CloseBracket => s.push(']'),
            Token::BeginEnv(e) => {
                s.push_str("\\begin{");
                s.push_str(e);
                s.push('}');
            }
            Token::EndEnv(e) => {
                s.push_str("\\end{");
                s.push_str(e);
                s.push('}');
            }
            Token::DisplayMath(m) => {
                s.push_str("$$");
                s.push_str(m);
                s.push_str("$$");
            }
            Token::BracketMath(m) => {
                s.push_str("\\[");
                s.push_str(m);
                s.push_str("\\]");
            }
        }
    }
    s.trim().to_string()
}

// ---------------------------------------------------------------------------
// Tabular parser
// ---------------------------------------------------------------------------

/// Parse a tabular environment body into rows of cells.
/// Rows are separated by `\\` and cells by `&`.
/// Returns a Vec of rows, where each row is a Vec of cell strings.
fn parse_tabular_body(tokens: &[Token], start: usize, end: usize) -> Vec<Vec<String>> {
    // First, collect the raw text of the tabular body
    let raw = collect_raw_text(tokens, start, end);

    // Split into rows on `\\` and then cells on `&`
    let mut rows: Vec<Vec<String>> = Vec::new();
    for row_text in raw.split("\\\\") {
        let trimmed = row_text.trim();
        // Skip \hline and empty rows
        if trimmed.is_empty() || trimmed == "\\hline" {
            continue;
        }
        // Remove any \hline at start or end of the row
        let cleaned = trimmed.replace("\\hline", "").trim().to_string();
        if cleaned.is_empty() {
            continue;
        }
        let cells: Vec<String> = cleaned.split('&').map(|c| c.trim().to_string()).collect();
        if cells.iter().all(|c| c.is_empty()) {
            continue;
        }
        rows.push(cells);
    }
    rows
}

// ---------------------------------------------------------------------------
// Strip inline formatting from text
// ---------------------------------------------------------------------------

/// Strip common LaTeX inline formatting commands, returning plain text.
/// Handles `\textbf{...}`, `\emph{...}`, `\textit{...}`, `\texttt{...}`, etc.
/// Also strips tilde `~` (non-breaking space) to a normal space.
fn strip_formatting(text: &str) -> String {
    text.replace('~', " ")
}

// ---------------------------------------------------------------------------
// Main parser: walk tokens and emit RDF triples
// ---------------------------------------------------------------------------

/// Parse a token stream and emit RDF triples into the store.
fn parse_tokens(tokens: &[Token], ctx: &mut ParseContext<'_>) -> ruddydoc_core::Result<()> {
    let mut i = 0;
    let mut text_buf = String::new();

    while i < tokens.len() {
        match &tokens[i] {
            Token::Command(cmd) => {
                match cmd.as_str() {
                    "documentclass" | "usepackage" | "author" | "date" | "maketitle"
                    | "centering" | "hline" | "newpage" | "clearpage" | "tableofcontents"
                    | "noindent" | "bigskip" | "medskip" | "smallskip" | "vspace" | "hspace"
                    | "newline" | "linebreak" | "pagebreak" => {
                        // Skip these commands and consume their optional/required arguments
                        i += 1;
                        // Consume optional [...]
                        let (_, new_i) = read_bracket_group(tokens, i);
                        i = new_i;
                        // Consume required {...}
                        if i < tokens.len() && tokens[i] == Token::OpenBrace {
                            let (_, new_i) = read_brace_group(tokens, i);
                            i = new_i;
                        }
                        continue;
                    }

                    "title" => {
                        // Flush any pending text as paragraph
                        flush_paragraph(&mut text_buf, ctx)?;
                        i += 1;
                        if i < tokens.len() && tokens[i] == Token::OpenBrace {
                            let (text, new_i) = read_brace_group(tokens, i);
                            i = new_i;
                            let iri = ctx.element_iri("title");
                            ctx.emit_element(&iri, ont::CLASS_TITLE)?;
                            ctx.set_text_content(&iri, &strip_formatting(&text))?;
                        }
                        continue;
                    }

                    "section" => {
                        flush_paragraph(&mut text_buf, ctx)?;
                        i += 1;
                        // optional star
                        // consume optional [...]
                        let (_, new_i) = read_bracket_group(tokens, i);
                        i = new_i;
                        if i < tokens.len() && tokens[i] == Token::OpenBrace {
                            let (text, new_i) = read_brace_group(tokens, i);
                            i = new_i;
                            let iri = ctx.element_iri("heading");
                            ctx.emit_element(&iri, ont::CLASS_SECTION_HEADER)?;
                            ctx.set_text_content(&iri, &strip_formatting(&text))?;
                            ctx.store.insert_literal(
                                &iri,
                                &ont::iri(ont::PROP_HEADING_LEVEL),
                                "1",
                                "integer",
                                ctx.doc_graph,
                            )?;
                        }
                        continue;
                    }

                    "subsection" => {
                        flush_paragraph(&mut text_buf, ctx)?;
                        i += 1;
                        let (_, new_i) = read_bracket_group(tokens, i);
                        i = new_i;
                        if i < tokens.len() && tokens[i] == Token::OpenBrace {
                            let (text, new_i) = read_brace_group(tokens, i);
                            i = new_i;
                            let iri = ctx.element_iri("heading");
                            ctx.emit_element(&iri, ont::CLASS_SECTION_HEADER)?;
                            ctx.set_text_content(&iri, &strip_formatting(&text))?;
                            ctx.store.insert_literal(
                                &iri,
                                &ont::iri(ont::PROP_HEADING_LEVEL),
                                "2",
                                "integer",
                                ctx.doc_graph,
                            )?;
                        }
                        continue;
                    }

                    "subsubsection" => {
                        flush_paragraph(&mut text_buf, ctx)?;
                        i += 1;
                        let (_, new_i) = read_bracket_group(tokens, i);
                        i = new_i;
                        if i < tokens.len() && tokens[i] == Token::OpenBrace {
                            let (text, new_i) = read_brace_group(tokens, i);
                            i = new_i;
                            let iri = ctx.element_iri("heading");
                            ctx.emit_element(&iri, ont::CLASS_SECTION_HEADER)?;
                            ctx.set_text_content(&iri, &strip_formatting(&text))?;
                            ctx.store.insert_literal(
                                &iri,
                                &ont::iri(ont::PROP_HEADING_LEVEL),
                                "3",
                                "integer",
                                ctx.doc_graph,
                            )?;
                        }
                        continue;
                    }

                    "paragraph" => {
                        flush_paragraph(&mut text_buf, ctx)?;
                        i += 1;
                        let (_, new_i) = read_bracket_group(tokens, i);
                        i = new_i;
                        if i < tokens.len() && tokens[i] == Token::OpenBrace {
                            let (text, new_i) = read_brace_group(tokens, i);
                            i = new_i;
                            let iri = ctx.element_iri("heading");
                            ctx.emit_element(&iri, ont::CLASS_SECTION_HEADER)?;
                            ctx.set_text_content(&iri, &strip_formatting(&text))?;
                            ctx.store.insert_literal(
                                &iri,
                                &ont::iri(ont::PROP_HEADING_LEVEL),
                                "4",
                                "integer",
                                ctx.doc_graph,
                            )?;
                        }
                        continue;
                    }

                    "label" => {
                        i += 1;
                        if i < tokens.len() && tokens[i] == Token::OpenBrace {
                            let (label_name, new_i) = read_brace_group(tokens, i);
                            i = new_i;
                            // Attach the label to the most recently emitted element
                            if let Some(iri) = &ctx.last_element_iri {
                                ctx.store.insert_literal(
                                    iri,
                                    &ont::iri(ont::PROP_LABEL_ID),
                                    &label_name,
                                    "string",
                                    ctx.doc_graph,
                                )?;
                                ctx.label_map.push((label_name, iri.clone()));
                            }
                        }
                        continue;
                    }

                    "ref" | "eqref" | "pageref" => {
                        i += 1;
                        if i < tokens.len() && tokens[i] == Token::OpenBrace {
                            let (ref_target, new_i) = read_brace_group(tokens, i);
                            i = new_i;
                            // Create a Reference element
                            let iri = ctx.element_iri("ref");
                            ctx.emit_element(&iri, ont::CLASS_REFERENCE)?;
                            ctx.set_text_content(&iri, &ref_target)?;
                            ctx.pending_refs.push((iri, ref_target));
                        }
                        continue;
                    }

                    "cite" => {
                        i += 1;
                        // optional [...]
                        let (_, new_i) = read_bracket_group(tokens, i);
                        i = new_i;
                        if i < tokens.len() && tokens[i] == Token::OpenBrace {
                            let (cite_key, new_i) = read_brace_group(tokens, i);
                            i = new_i;
                            let iri = ctx.element_iri("ref");
                            ctx.emit_element(&iri, ont::CLASS_REFERENCE)?;
                            ctx.store.insert_literal(
                                &iri,
                                &ont::iri(ont::PROP_CITATION_KEY),
                                &cite_key,
                                "string",
                                ctx.doc_graph,
                            )?;
                            ctx.pending_refs.push((iri, cite_key));
                        }
                        continue;
                    }

                    "footnote" => {
                        i += 1;
                        if i < tokens.len() && tokens[i] == Token::OpenBrace {
                            let (text, new_i) = read_brace_group(tokens, i);
                            i = new_i;
                            let iri = ctx.element_iri("footnote");
                            ctx.emit_element(&iri, ont::CLASS_FOOTNOTE)?;
                            ctx.set_text_content(&iri, &strip_formatting(&text))?;
                        }
                        continue;
                    }

                    "caption" => {
                        i += 1;
                        if i < tokens.len() && tokens[i] == Token::OpenBrace {
                            let (text, new_i) = read_brace_group(tokens, i);
                            i = new_i;
                            let iri = ctx.element_iri("caption");
                            ctx.emit_element(&iri, ont::CLASS_CAPTION)?;
                            ctx.set_text_content(&iri, &strip_formatting(&text))?;

                            // If parent is a figure/picture, link caption
                            if let Some(parent) = ctx.parent_stack.last() {
                                ctx.store.insert_triple_into(
                                    parent,
                                    &ont::iri(ont::PROP_HAS_CAPTION),
                                    &iri,
                                    ctx.doc_graph,
                                )?;
                            }
                        }
                        continue;
                    }

                    "includegraphics" => {
                        i += 1;
                        // optional [options]
                        let (_, new_i) = read_bracket_group(tokens, i);
                        i = new_i;
                        if i < tokens.len() && tokens[i] == Token::OpenBrace {
                            let (path, new_i) = read_brace_group(tokens, i);
                            i = new_i;
                            let iri = ctx.element_iri("picture");
                            ctx.emit_element(&iri, ont::CLASS_PICTURE_ELEMENT)?;
                            ctx.store.insert_literal(
                                &iri,
                                &ont::iri(ont::PROP_LINK_TARGET),
                                &path,
                                "string",
                                ctx.doc_graph,
                            )?;
                            // Infer format from filename extension
                            if let Some(ext) = path
                                .rsplit('.')
                                .next()
                                .map(|s| s.to_lowercase())
                                .filter(|ext| {
                                    matches!(
                                        ext.as_str(),
                                        "png"
                                            | "jpg"
                                            | "jpeg"
                                            | "gif"
                                            | "svg"
                                            | "webp"
                                            | "tiff"
                                            | "bmp"
                                            | "pdf"
                                            | "eps"
                                    )
                                })
                            {
                                ctx.store.insert_literal(
                                    &iri,
                                    &ont::iri(ont::PROP_PICTURE_FORMAT),
                                    &ext,
                                    "string",
                                    ctx.doc_graph,
                                )?;
                            }
                        }
                        continue;
                    }

                    "item" => {
                        // Flush any pending text from the previous item
                        flush_paragraph(&mut text_buf, ctx)?;
                        i += 1;
                        // The item text is everything until the next \item, \end, or end of tokens.
                        // We accumulate text into the buffer and let it be flushed as a ListItem.
                        // Read item text inline until we hit another \item or \end{...}
                        let mut item_text = String::new();
                        while i < tokens.len() {
                            match &tokens[i] {
                                Token::Command(c) if c == "item" => break,
                                Token::EndEnv(_) => break,
                                Token::Text(t) => {
                                    item_text.push_str(t);
                                    i += 1;
                                }
                                Token::Command(c) => {
                                    match c.as_str() {
                                        "textbf" | "textit" | "emph" | "texttt" | "textsf"
                                        | "textrm" | "textsc" | "underline" => {
                                            // Strip formatting: skip command, read brace group
                                            i += 1;
                                            if i < tokens.len() && tokens[i] == Token::OpenBrace {
                                                let (inner, new_i) = read_brace_group(tokens, i);
                                                i = new_i;
                                                item_text.push_str(&inner);
                                            }
                                        }
                                        _ => {
                                            i += 1;
                                            // Consume optional arguments
                                            if i < tokens.len() && tokens[i] == Token::OpenBracket {
                                                let (_, new_i) = read_bracket_group(tokens, i);
                                                i = new_i;
                                            }
                                            if i < tokens.len() && tokens[i] == Token::OpenBrace {
                                                let (_, new_i) = read_brace_group(tokens, i);
                                                i = new_i;
                                            }
                                        }
                                    }
                                }
                                Token::OpenBrace => {
                                    let (inner, new_i) = read_brace_group(tokens, i);
                                    i = new_i;
                                    item_text.push_str(&inner);
                                }
                                _ => {
                                    i += 1;
                                }
                            }
                        }
                        let trimmed = strip_formatting(item_text.trim());
                        if !trimmed.is_empty() {
                            let iri = ctx.element_iri("listitem");
                            ctx.emit_element(&iri, ont::CLASS_LIST_ITEM)?;
                            ctx.set_text_content(&iri, &trimmed)?;
                        }
                        continue;
                    }

                    // Inline formatting commands: strip the command, keep the content
                    "textbf" | "textit" | "emph" | "texttt" | "textsf" | "textrm" | "textsc"
                    | "underline" => {
                        i += 1;
                        if i < tokens.len() && tokens[i] == Token::OpenBrace {
                            let (inner, new_i) = read_brace_group(tokens, i);
                            i = new_i;
                            text_buf.push_str(&inner);
                        }
                        continue;
                    }

                    _ => {
                        // Unknown command: skip it and any arguments
                        i += 1;
                        continue;
                    }
                }
            }

            Token::BeginEnv(env) => {
                flush_paragraph(&mut text_buf, ctx)?;
                match env.as_str() {
                    "document" => {
                        // Just enter the document environment
                        i += 1;
                        continue;
                    }

                    "abstract" => {
                        i += 1;
                        let (end_pos, next_i) = find_env_end(tokens, i, "abstract");
                        let raw = collect_raw_text(tokens, i, end_pos);
                        let text = strip_formatting(raw.trim());
                        if !text.is_empty() {
                            let iri = ctx.element_iri("paragraph");
                            ctx.emit_element(&iri, ont::CLASS_PARAGRAPH)?;
                            ctx.set_text_content(&iri, &text)?;
                        }
                        i = next_i;
                        continue;
                    }

                    "itemize" => {
                        i += 1;
                        let list_iri = ctx.element_iri("list");
                        ctx.emit_element(&list_iri, ont::CLASS_UNORDERED_LIST)?;
                        ctx.parent_stack.push(list_iri);
                        // Continue parsing; items will be children of this list
                        continue;
                    }

                    "enumerate" => {
                        i += 1;
                        let list_iri = ctx.element_iri("list");
                        ctx.emit_element(&list_iri, ont::CLASS_ORDERED_LIST)?;
                        ctx.parent_stack.push(list_iri);
                        continue;
                    }

                    "table" => {
                        i += 1;
                        // Consume optional [placement]
                        let (_, new_i) = read_bracket_group(tokens, i);
                        i = new_i;
                        let table_iri = ctx.element_iri("table");
                        ctx.emit_element(&table_iri, ont::CLASS_TABLE_ELEMENT)?;
                        ctx.parent_stack.push(table_iri);
                        continue;
                    }

                    "tabular" => {
                        i += 1;
                        // Read column spec {|l|c|r|}
                        if i < tokens.len() && tokens[i] == Token::OpenBrace {
                            let (_, new_i) = read_brace_group(tokens, i);
                            i = new_i;
                        }
                        // Find the end of the tabular environment
                        let (end_pos, next_i) = find_env_end(tokens, i, "tabular");
                        // Parse the tabular body into rows and cells
                        let rows = parse_tabular_body(tokens, i, end_pos);

                        // Determine the parent table IRI
                        let table_iri = ctx.parent_stack.last().cloned();

                        for (row_idx, row) in rows.iter().enumerate() {
                            for (col_idx, cell_text) in row.iter().enumerate() {
                                let cell_iri = ruddydoc_core::element_iri(
                                    ctx.doc_hash,
                                    &format!("cell-{row_idx}-{col_idx}"),
                                );
                                let rdf_type = ont::rdf_iri("type");
                                let g = ctx.doc_graph;

                                ctx.store.insert_triple_into(
                                    &cell_iri,
                                    &rdf_type,
                                    &ont::iri(ont::CLASS_TABLE_CELL),
                                    g,
                                )?;

                                if let Some(ref tbl) = table_iri {
                                    ctx.store.insert_triple_into(
                                        tbl,
                                        &ont::iri(ont::PROP_HAS_CELL),
                                        &cell_iri,
                                        g,
                                    )?;
                                }

                                ctx.store.insert_literal(
                                    &cell_iri,
                                    &ont::iri(ont::PROP_CELL_ROW),
                                    &row_idx.to_string(),
                                    "integer",
                                    g,
                                )?;
                                ctx.store.insert_literal(
                                    &cell_iri,
                                    &ont::iri(ont::PROP_CELL_COLUMN),
                                    &col_idx.to_string(),
                                    "integer",
                                    g,
                                )?;
                                ctx.store.insert_literal(
                                    &cell_iri,
                                    &ont::iri(ont::PROP_CELL_TEXT),
                                    &strip_formatting(cell_text),
                                    "string",
                                    g,
                                )?;
                            }
                        }

                        // Set row/column counts on the table
                        if let Some(ref tbl) = table_iri {
                            let row_count = rows.len();
                            let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
                            ctx.store.insert_literal(
                                tbl,
                                &ont::iri(ont::PROP_ROW_COUNT),
                                &row_count.to_string(),
                                "integer",
                                ctx.doc_graph,
                            )?;
                            ctx.store.insert_literal(
                                tbl,
                                &ont::iri(ont::PROP_COLUMN_COUNT),
                                &col_count.to_string(),
                                "integer",
                                ctx.doc_graph,
                            )?;
                        }

                        i = next_i;
                        continue;
                    }

                    "figure" => {
                        i += 1;
                        // Consume optional [placement]
                        let (_, new_i) = read_bracket_group(tokens, i);
                        i = new_i;
                        let fig_iri = ctx.element_iri("figure");
                        ctx.emit_element(&fig_iri, ont::CLASS_PICTURE_ELEMENT)?;
                        ctx.parent_stack.push(fig_iri);
                        continue;
                    }

                    "equation" | "equation*" | "align" | "align*" | "gather" | "gather*"
                    | "math" | "displaymath" => {
                        i += 1;
                        let env_name_owned = env.clone();
                        let (end_pos, next_i) = find_env_end(tokens, i, &env_name_owned);
                        let raw = collect_raw_text(tokens, i, end_pos);
                        let iri = ctx.element_iri("formula");
                        ctx.emit_element(&iri, ont::CLASS_FORMULA)?;
                        ctx.set_text_content(&iri, &raw)?;
                        i = next_i;
                        continue;
                    }

                    "verbatim" => {
                        i += 1;
                        let (end_pos, next_i) = find_env_end(tokens, i, "verbatim");
                        let raw = collect_raw_text(tokens, i, end_pos);
                        let iri = ctx.element_iri("code");
                        ctx.emit_element(&iri, ont::CLASS_CODE)?;
                        ctx.set_text_content(&iri, &raw)?;
                        i = next_i;
                        continue;
                    }

                    "lstlisting" => {
                        i += 1;
                        // Optional [language=Python] argument
                        let (opt, new_i) = read_bracket_group(tokens, i);
                        i = new_i;
                        let (end_pos, next_i) = find_env_end(tokens, i, "lstlisting");
                        let raw = collect_raw_text(tokens, i, end_pos);
                        let iri = ctx.element_iri("code");
                        ctx.emit_element(&iri, ont::CLASS_CODE)?;
                        ctx.set_text_content(&iri, &raw)?;

                        // Extract language from options
                        if let Some(opts) = opt
                            && let Some(lang) = extract_key_value(&opts, "language")
                        {
                            ctx.store.insert_literal(
                                &iri,
                                &ont::iri(ont::PROP_CODE_LANGUAGE),
                                &lang,
                                "string",
                                ctx.doc_graph,
                            )?;
                        }

                        i = next_i;
                        continue;
                    }

                    _ => {
                        // Unknown environment: skip its body
                        i += 1;
                        let env_name_owned = env.clone();
                        let (_, next_i) = find_env_end(tokens, i, &env_name_owned);
                        i = next_i;
                        continue;
                    }
                }
            }

            Token::EndEnv(env) => {
                flush_paragraph(&mut text_buf, ctx)?;
                match env.as_str() {
                    "itemize" | "enumerate" | "table" | "figure" => {
                        ctx.parent_stack.pop();
                    }
                    "document" => {
                        // End of document
                    }
                    _ => {}
                }
                i += 1;
                continue;
            }

            Token::DisplayMath(math) => {
                flush_paragraph(&mut text_buf, ctx)?;
                let iri = ctx.element_iri("formula");
                ctx.emit_element(&iri, ont::CLASS_FORMULA)?;
                ctx.set_text_content(&iri, math)?;
                i += 1;
                continue;
            }

            Token::BracketMath(math) => {
                flush_paragraph(&mut text_buf, ctx)?;
                let iri = ctx.element_iri("formula");
                ctx.emit_element(&iri, ont::CLASS_FORMULA)?;
                ctx.set_text_content(&iri, math)?;
                i += 1;
                continue;
            }

            Token::Text(t) => {
                // LaTeX separates paragraphs by blank lines (double newline).
                // Split text on blank lines and emit paragraphs for completed
                // chunks while keeping the last chunk in the buffer.
                let combined = format!("{text_buf}{t}");
                text_buf.clear();

                // Split on blank lines (two or more consecutive newlines)
                let parts: Vec<&str> = combined.split("\n\n").collect();
                if parts.len() > 1 {
                    // Emit all but the last part as paragraphs
                    for part in &parts[..parts.len() - 1] {
                        let trimmed = strip_formatting(part.trim());
                        if !trimmed.is_empty() {
                            let iri = ctx.element_iri("paragraph");
                            ctx.emit_element(&iri, ont::CLASS_PARAGRAPH)?;
                            ctx.set_text_content(&iri, &trimmed)?;
                        }
                    }
                    // Keep the last part in the buffer
                    text_buf.push_str(parts[parts.len() - 1]);
                } else {
                    text_buf.push_str(&combined);
                }

                i += 1;
                continue;
            }

            Token::OpenBrace | Token::CloseBrace | Token::OpenBracket | Token::CloseBracket => {
                // Stray braces/brackets: skip
                i += 1;
                continue;
            }
        }
    }

    // Flush any remaining text
    flush_paragraph(&mut text_buf, ctx)?;

    Ok(())
}

/// Flush accumulated text buffer as a Paragraph element if non-empty.
fn flush_paragraph(text_buf: &mut String, ctx: &mut ParseContext<'_>) -> ruddydoc_core::Result<()> {
    let text = strip_formatting(text_buf.trim());
    text_buf.clear();
    if !text.is_empty() {
        let iri = ctx.element_iri("paragraph");
        ctx.emit_element(&iri, ont::CLASS_PARAGRAPH)?;
        ctx.set_text_content(&iri, &text)?;
    }
    Ok(())
}

/// Extract a value from a comma-separated `key=value` option string.
fn extract_key_value(opts: &str, key: &str) -> Option<String> {
    for part in opts.split(',') {
        let part = part.trim();
        if let Some(val) = part.strip_prefix(key) {
            let val = val.trim_start_matches('=').trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// LaTeX backend
// ---------------------------------------------------------------------------

/// LaTeX document backend.
pub struct LatexBackend;

impl LatexBackend {
    /// Create a new LaTeX backend instance.
    pub fn new() -> Self {
        Self
    }
}

impl Default for LatexBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentBackend for LatexBackend {
    fn supported_formats(&self) -> &[InputFormat] {
        &[InputFormat::Latex]
    }

    fn supports_pagination(&self) -> bool {
        false
    }

    fn is_valid(&self, source: &DocumentSource) -> bool {
        match source {
            DocumentSource::File(path) => {
                matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("tex" | "latex")
                )
            }
            DocumentSource::Stream { name, .. } => {
                name.ends_with(".tex") || name.ends_with(".latex")
            }
        }
    }

    fn parse(
        &self,
        source: &DocumentSource,
        store: &dyn DocumentStore,
        doc_graph: &str,
    ) -> ruddydoc_core::Result<DocumentMeta> {
        // Read the source content
        let (content, file_path, file_name) = match source {
            DocumentSource::File(path) => {
                let content = std::fs::read_to_string(path)?;
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "unknown".to_string());
                (content, Some(path.clone()), name)
            }
            DocumentSource::Stream { name, data } => {
                let content = String::from_utf8(data.clone())?;
                (content, None, name.clone())
            }
        };

        let file_size = content.len() as u64;
        let hash_str = compute_hash(content.as_bytes());
        let doc_hash = DocumentHash(hash_str.clone());

        // Create the document node
        let doc_iri = ruddydoc_core::doc_iri(&hash_str);
        let rdf_type = ont::rdf_iri("type");
        let g = doc_graph;

        store.insert_triple_into(&doc_iri, &rdf_type, &ont::iri(ont::CLASS_DOCUMENT), g)?;
        store.insert_literal(
            &doc_iri,
            &ont::iri(ont::PROP_SOURCE_FORMAT),
            "latex",
            "string",
            g,
        )?;
        store.insert_literal(
            &doc_iri,
            &ont::iri(ont::PROP_DOCUMENT_HASH),
            &hash_str,
            "string",
            g,
        )?;
        store.insert_literal(
            &doc_iri,
            &ont::iri(ont::PROP_FILE_NAME),
            &file_name,
            "string",
            g,
        )?;
        store.insert_literal(
            &doc_iri,
            &ont::iri(ont::PROP_FILE_SIZE),
            &file_size.to_string(),
            "integer",
            g,
        )?;

        // Tokenize and parse
        let tokens = tokenize(&content);
        let mut ctx = ParseContext::new(store, g, &hash_str);
        parse_tokens(&tokens, &mut ctx)?;

        // Resolve cross-references
        ctx.resolve_refs()?;

        Ok(DocumentMeta {
            file_path,
            hash: doc_hash,
            format: InputFormat::Latex,
            file_size,
            page_count: None,
            language: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ruddydoc_graph::OxigraphStore;

    fn parse_latex(latex: &str) -> ruddydoc_core::Result<(OxigraphStore, DocumentMeta, String)> {
        let store = OxigraphStore::new()?;
        let backend = LatexBackend::new();
        let source = DocumentSource::Stream {
            name: "test.tex".to_string(),
            data: latex.as_bytes().to_vec(),
        };

        let hash_str = compute_hash(latex.as_bytes());
        let doc_graph = ruddydoc_core::doc_iri(&hash_str);

        let meta = backend.parse(&source, &store, &doc_graph)?;
        Ok((store, meta, doc_graph))
    }

    // -------------------------------------------------------------------
    // Basic document structure
    // -------------------------------------------------------------------

    #[test]
    fn parse_title() -> ruddydoc_core::Result<()> {
        let tex = r"\title{My Document}";
        let (store, _meta, graph) = parse_latex(tex)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?t a <{}>. \
                 ?t <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_TITLE),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("My Document"));
        Ok(())
    }

    #[test]
    fn parse_sections() -> ruddydoc_core::Result<()> {
        let tex = r"\section{Introduction}
\subsection{Background}
\subsubsection{Details}
\paragraph{Note}";
        let (store, _meta, graph) = parse_latex(tex)?;

        let sparql = format!(
            "SELECT ?text ?level WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?h a <{}>. \
                 ?h <{}> ?text. \
                 ?h <{}> ?level \
               }} \
             }} ORDER BY ?level",
            ont::iri(ont::CLASS_SECTION_HEADER),
            ont::iri(ont::PROP_TEXT_CONTENT),
            ont::iri(ont::PROP_HEADING_LEVEL),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 4);

        // Verify heading levels 1-4
        let levels: Vec<&str> = rows.iter().map(|r| r["level"].as_str().unwrap()).collect();
        assert!(levels[0].contains('1'));
        assert!(levels[1].contains('2'));
        assert!(levels[2].contains('3'));
        assert!(levels[3].contains('4'));
        Ok(())
    }

    #[test]
    fn parse_paragraphs() -> ruddydoc_core::Result<()> {
        let tex = r"\begin{document}

This is paragraph one.

This is paragraph two.

\end{document}";
        let (store, _meta, graph) = parse_latex(tex)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?p a <{}>. \
                 ?p <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_PARAGRAPH),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert!(
            rows.len() >= 2,
            "expected at least 2 paragraphs, got {}",
            rows.len()
        );
        Ok(())
    }

    #[test]
    fn strip_inline_formatting() -> ruddydoc_core::Result<()> {
        let tex = r"\section{A \textbf{bold} and \emph{italic} heading}";
        let (store, _meta, graph) = parse_latex(tex)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?h a <{}>. \
                 ?h <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_SECTION_HEADER),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let text = rows[0]["text"].as_str().expect("text");
        // Should contain the plain text without formatting commands
        assert!(text.contains("bold"));
        assert!(text.contains("italic"));
        // Should NOT contain \textbf or \emph
        assert!(!text.contains("\\textbf"));
        assert!(!text.contains("\\emph"));
        Ok(())
    }

    // -------------------------------------------------------------------
    // Lists
    // -------------------------------------------------------------------

    #[test]
    fn parse_unordered_list() -> ruddydoc_core::Result<()> {
        let tex = r"\begin{itemize}
\item First item
\item Second item
\item Third item
\end{itemize}";
        let (store, _meta, graph) = parse_latex(tex)?;

        // Check that we have an UnorderedList
        let sparql_list = format!(
            "ASK {{ GRAPH <{graph}> {{ ?l a <{}> }} }}",
            ont::iri(ont::CLASS_UNORDERED_LIST),
        );
        let result = store.query_to_json(&sparql_list)?;
        assert_eq!(result, serde_json::Value::Bool(true));

        // Check that we have 3 list items
        let sparql_items = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?li a <{}>. \
                 ?li <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_LIST_ITEM),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql_items)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 3);
        Ok(())
    }

    #[test]
    fn parse_ordered_list() -> ruddydoc_core::Result<()> {
        let tex = r"\begin{enumerate}
\item Alpha
\item Beta
\item Gamma
\end{enumerate}";
        let (store, _meta, graph) = parse_latex(tex)?;

        let sparql = format!(
            "ASK {{ GRAPH <{graph}> {{ ?l a <{}> }} }}",
            ont::iri(ont::CLASS_ORDERED_LIST),
        );
        let result = store.query_to_json(&sparql)?;
        assert_eq!(result, serde_json::Value::Bool(true));

        // Check items are children of the list
        let sparql_items = format!(
            "SELECT ?item WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?item a <{}>. \
                 ?item <{}> ?parent. \
                 ?parent a <{}> \
               }} \
             }}",
            ont::iri(ont::CLASS_LIST_ITEM),
            ont::iri(ont::PROP_PARENT_ELEMENT),
            ont::iri(ont::CLASS_ORDERED_LIST),
        );
        let result = store.query_to_json(&sparql_items)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 3);
        Ok(())
    }

    // -------------------------------------------------------------------
    // Tables
    // -------------------------------------------------------------------

    #[test]
    fn parse_tabular() -> ruddydoc_core::Result<()> {
        let tex = r"\begin{table}[h]
\begin{tabular}{|l|c|r|}
\hline
Name & Score & Rank \\
\hline
Alice & 95 & 1 \\
Bob & 87 & 2 \\
\hline
\end{tabular}
\end{table}";
        let (store, _meta, graph) = parse_latex(tex)?;

        // Check that we have a TableElement
        let sparql_table = format!(
            "ASK {{ GRAPH <{graph}> {{ ?t a <{}> }} }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
        );
        let result = store.query_to_json(&sparql_table)?;
        assert_eq!(result, serde_json::Value::Bool(true));

        // Check cells exist
        let sparql_cells = format!(
            "SELECT ?text ?row ?col WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> ?text. \
                 ?c <{}> ?row. \
                 ?c <{}> ?col \
               }} \
             }} ORDER BY ?row ?col",
            ont::iri(ont::CLASS_TABLE_CELL),
            ont::iri(ont::PROP_CELL_TEXT),
            ont::iri(ont::PROP_CELL_ROW),
            ont::iri(ont::PROP_CELL_COLUMN),
        );
        let result = store.query_to_json(&sparql_cells)?;
        let rows = result.as_array().expect("expected array");
        // 3 rows x 3 columns = 9 cells
        assert_eq!(rows.len(), 9, "expected 9 cells, got {}", rows.len());

        // Check row/column counts on the table
        let sparql_counts = format!(
            "SELECT ?rc ?cc WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?t a <{}>. \
                 ?t <{}> ?rc. \
                 ?t <{}> ?cc \
               }} \
             }}",
            ont::iri(ont::CLASS_TABLE_ELEMENT),
            ont::iri(ont::PROP_ROW_COUNT),
            ont::iri(ont::PROP_COLUMN_COUNT),
        );
        let result = store.query_to_json(&sparql_counts)?;
        let count_rows = result.as_array().expect("expected array");
        assert_eq!(count_rows.len(), 1);

        let rc = count_rows[0]["rc"].as_str().expect("rc");
        assert!(rc.contains('3'));
        let cc = count_rows[0]["cc"].as_str().expect("cc");
        assert!(cc.contains('3'));
        Ok(())
    }

    // -------------------------------------------------------------------
    // Equations / Math
    // -------------------------------------------------------------------

    #[test]
    fn parse_equation_env() -> ruddydoc_core::Result<()> {
        let tex = r"\begin{equation}
E = mc^2
\end{equation}";
        let (store, _meta, graph) = parse_latex(tex)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?f a <{}>. \
                 ?f <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_FORMULA),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("E = mc^2"));
        Ok(())
    }

    #[test]
    fn parse_display_math() -> ruddydoc_core::Result<()> {
        let tex = r"$$F = ma$$";
        let (store, _meta, graph) = parse_latex(tex)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?f a <{}>. \
                 ?f <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_FORMULA),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("F = ma"));
        Ok(())
    }

    #[test]
    fn parse_bracket_math() -> ruddydoc_core::Result<()> {
        let tex = r"\[\sum_{i=1}^{n} x_i\]";
        let (store, _meta, graph) = parse_latex(tex)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?f a <{}>. \
                 ?f <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_FORMULA),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        Ok(())
    }

    // -------------------------------------------------------------------
    // Figures and images
    // -------------------------------------------------------------------

    #[test]
    fn parse_figure_with_caption() -> ruddydoc_core::Result<()> {
        let tex = r"\begin{figure}[h]
\includegraphics{photo.png}
\caption{A test photo}
\end{figure}";
        let (store, _meta, graph) = parse_latex(tex)?;

        // Check PictureElement from figure env
        let sparql_fig = format!(
            "ASK {{ GRAPH <{graph}> {{ ?p a <{}> }} }}",
            ont::iri(ont::CLASS_PICTURE_ELEMENT),
        );
        let result = store.query_to_json(&sparql_fig)?;
        assert_eq!(result, serde_json::Value::Bool(true));

        // Check Caption
        let sparql_cap = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_CAPTION),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql_cap)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("A test photo"));

        // Check hasCaption link
        let sparql_link = format!(
            "ASK {{ GRAPH <{graph}> {{ ?fig <{}> ?cap }} }}",
            ont::iri(ont::PROP_HAS_CAPTION),
        );
        let result = store.query_to_json(&sparql_link)?;
        assert_eq!(result, serde_json::Value::Bool(true));
        Ok(())
    }

    #[test]
    fn parse_includegraphics() -> ruddydoc_core::Result<()> {
        let tex = r"\includegraphics[width=5cm]{diagram.svg}";
        let (store, _meta, graph) = parse_latex(tex)?;

        let sparql = format!(
            "SELECT ?target ?fmt WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?p a <{}>. \
                 ?p <{}> ?target. \
                 ?p <{}> ?fmt \
               }} \
             }}",
            ont::iri(ont::CLASS_PICTURE_ELEMENT),
            ont::iri(ont::PROP_LINK_TARGET),
            ont::iri(ont::PROP_PICTURE_FORMAT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let target = rows[0]["target"].as_str().expect("target");
        assert!(target.contains("diagram.svg"));

        let fmt = rows[0]["fmt"].as_str().expect("fmt");
        assert!(fmt.contains("svg"));
        Ok(())
    }

    // -------------------------------------------------------------------
    // Cross-references
    // -------------------------------------------------------------------

    #[test]
    fn parse_label_and_ref() -> ruddydoc_core::Result<()> {
        let tex = r"\section{Introduction}
\label{sec:intro}

See Section~\ref{sec:intro} for details.";
        let (store, _meta, graph) = parse_latex(tex)?;

        // Check that the section has a labelId
        let sparql_label = format!(
            "SELECT ?label WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?h a <{}>. \
                 ?h <{}> ?label \
               }} \
             }}",
            ont::iri(ont::CLASS_SECTION_HEADER),
            ont::iri(ont::PROP_LABEL_ID),
        );
        let result = store.query_to_json(&sparql_label)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let label = rows[0]["label"].as_str().expect("label");
        assert!(label.contains("sec:intro"));

        // Check that a Reference element exists that refersTo the section
        let sparql_ref = format!(
            "ASK {{ GRAPH <{graph}> {{ ?ref <{}> ?target }} }}",
            ont::iri(ont::PROP_REFERS_TO),
        );
        let result = store.query_to_json(&sparql_ref)?;
        assert_eq!(result, serde_json::Value::Bool(true));
        Ok(())
    }

    #[test]
    fn parse_cite() -> ruddydoc_core::Result<()> {
        let tex = r"\cite{knuth1984}";
        let (store, _meta, graph) = parse_latex(tex)?;

        let sparql = format!(
            "SELECT ?key WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?r a <{}>. \
                 ?r <{}> ?key \
               }} \
             }}",
            ont::iri(ont::CLASS_REFERENCE),
            ont::iri(ont::PROP_CITATION_KEY),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let key = rows[0]["key"].as_str().expect("key");
        assert!(key.contains("knuth1984"));
        Ok(())
    }

    // -------------------------------------------------------------------
    // Code blocks
    // -------------------------------------------------------------------

    #[test]
    fn parse_verbatim() -> ruddydoc_core::Result<()> {
        let tex = r"\begin{verbatim}
fn main() {}
\end{verbatim}";
        let (store, _meta, graph) = parse_latex(tex)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_CODE),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("fn main()"));
        Ok(())
    }

    #[test]
    fn parse_lstlisting_with_language() -> ruddydoc_core::Result<()> {
        let tex = r#"\begin{lstlisting}[language=Python]
def hello():
    print("Hello")
\end{lstlisting}"#;
        let (store, _meta, graph) = parse_latex(tex)?;

        let sparql = format!(
            "SELECT ?text ?lang WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?c a <{}>. \
                 ?c <{}> ?text. \
                 ?c <{}> ?lang \
               }} \
             }}",
            ont::iri(ont::CLASS_CODE),
            ont::iri(ont::PROP_TEXT_CONTENT),
            ont::iri(ont::PROP_CODE_LANGUAGE),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("def hello()"));

        let lang = rows[0]["lang"].as_str().expect("lang");
        assert!(lang.contains("Python"));
        Ok(())
    }

    // -------------------------------------------------------------------
    // Footnotes
    // -------------------------------------------------------------------

    #[test]
    fn parse_footnote() -> ruddydoc_core::Result<()> {
        let tex = r"\footnote{This is a footnote.}";
        let (store, _meta, graph) = parse_latex(tex)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?f a <{}>. \
                 ?f <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_FOOTNOTE),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);

        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("This is a footnote."));
        Ok(())
    }

    // -------------------------------------------------------------------
    // Abstract
    // -------------------------------------------------------------------

    #[test]
    fn parse_abstract() -> ruddydoc_core::Result<()> {
        let tex = r"\begin{abstract}
This is the abstract text.
\end{abstract}";
        let (store, _meta, graph) = parse_latex(tex)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?p a <{}>. \
                 ?p <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_PARAGRAPH),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert!(
            rows.iter().any(|r| {
                r["text"]
                    .as_str()
                    .map_or(false, |t| t.contains("abstract text"))
            }),
            "expected a paragraph containing 'abstract text'"
        );
        Ok(())
    }

    // -------------------------------------------------------------------
    // Comments
    // -------------------------------------------------------------------

    #[test]
    fn comments_are_stripped() -> ruddydoc_core::Result<()> {
        let tex = r"% This is a comment
\section{Visible}
% Another comment";
        let (store, _meta, graph) = parse_latex(tex)?;

        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?h a <{}>. \
                 ?h <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_SECTION_HEADER),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let text = rows[0]["text"].as_str().expect("text");
        assert!(text.contains("Visible"));
        assert!(!text.contains("comment"));
        Ok(())
    }

    // -------------------------------------------------------------------
    // Document metadata
    // -------------------------------------------------------------------

    #[test]
    fn document_metadata() -> ruddydoc_core::Result<()> {
        let tex = r"\title{Test}";
        let (_store, meta, _graph) = parse_latex(tex)?;

        assert_eq!(meta.format, InputFormat::Latex);
        assert!(meta.page_count.is_none());
        assert!(!meta.hash.0.is_empty());
        Ok(())
    }

    #[test]
    fn document_node_has_source_format() -> ruddydoc_core::Result<()> {
        let tex = r"\section{Hello}";
        let (store, meta, graph) = parse_latex(tex)?;

        let doc_iri = ruddydoc_core::doc_iri(&meta.hash.0);
        let sparql = format!(
            "SELECT ?fmt WHERE {{ \
               GRAPH <{graph}> {{ \
                 <{doc_iri}> <{}> ?fmt \
               }} \
             }}",
            ont::iri(ont::PROP_SOURCE_FORMAT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert_eq!(rows.len(), 1);
        let fmt = rows[0]["fmt"].as_str().expect("fmt");
        assert!(fmt.contains("latex"));
        Ok(())
    }

    // -------------------------------------------------------------------
    // Reading order
    // -------------------------------------------------------------------

    #[test]
    fn reading_order_is_sequential() -> ruddydoc_core::Result<()> {
        let tex = r"\section{Heading}

Paragraph one.

Paragraph two.";
        let (store, _meta, graph) = parse_latex(tex)?;

        let sparql = format!(
            "SELECT ?el ?order WHERE {{ \
               GRAPH <{graph}> {{ \
                 ?el <{}> ?order \
               }} \
             }} ORDER BY ?order",
            ont::iri(ont::PROP_READING_ORDER),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert!(rows.len() >= 3, "expected at least 3 elements");
        Ok(())
    }

    // -------------------------------------------------------------------
    // Backend trait methods
    // -------------------------------------------------------------------

    #[test]
    fn is_valid_tex() {
        let backend = LatexBackend::new();
        let source = DocumentSource::File(std::path::PathBuf::from("test.tex"));
        assert!(backend.is_valid(&source));
    }

    #[test]
    fn is_valid_latex_ext() {
        let backend = LatexBackend::new();
        let source = DocumentSource::File(std::path::PathBuf::from("test.latex"));
        assert!(backend.is_valid(&source));
    }

    #[test]
    fn is_not_valid_md() {
        let backend = LatexBackend::new();
        let source = DocumentSource::File(std::path::PathBuf::from("test.md"));
        assert!(!backend.is_valid(&source));
    }

    #[test]
    fn is_valid_stream() {
        let backend = LatexBackend::new();
        let source = DocumentSource::Stream {
            name: "paper.tex".to_string(),
            data: Vec::new(),
        };
        assert!(backend.is_valid(&source));
    }

    #[test]
    fn supported_formats_is_latex() {
        let backend = LatexBackend::new();
        assert_eq!(backend.supported_formats(), &[InputFormat::Latex]);
    }

    #[test]
    fn does_not_support_pagination() {
        let backend = LatexBackend::new();
        assert!(!backend.supports_pagination());
    }

    // -------------------------------------------------------------------
    // Full sample document fixture
    // -------------------------------------------------------------------

    #[test]
    fn parse_sample_fixture() -> ruddydoc_core::Result<()> {
        let fixture_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.tex");
        let content = std::fs::read_to_string(&fixture_path)?;

        let store = OxigraphStore::new()?;
        let backend = LatexBackend::new();
        let source = DocumentSource::Stream {
            name: "sample.tex".to_string(),
            data: content.as_bytes().to_vec(),
        };

        let hash_str = compute_hash(content.as_bytes());
        let doc_graph = ruddydoc_core::doc_iri(&hash_str);

        let meta = backend.parse(&source, &store, &doc_graph)?;
        assert_eq!(meta.format, InputFormat::Latex);

        // Should have sections
        let sparql = format!(
            "SELECT ?text WHERE {{ \
               GRAPH <{doc_graph}> {{ \
                 ?h a <{}>. \
                 ?h <{}> ?text \
               }} \
             }}",
            ont::iri(ont::CLASS_SECTION_HEADER),
            ont::iri(ont::PROP_TEXT_CONTENT),
        );
        let result = store.query_to_json(&sparql)?;
        let rows = result.as_array().expect("expected array");
        assert!(
            rows.len() >= 5,
            "expected at least 5 section headers, got {}",
            rows.len()
        );

        // Should have lists
        let sparql_lists = format!(
            "SELECT ?l WHERE {{ \
               GRAPH <{doc_graph}> {{ \
                 {{ ?l a <{}> }} UNION {{ ?l a <{}> }} \
               }} \
             }}",
            ont::iri(ont::CLASS_UNORDERED_LIST),
            ont::iri(ont::CLASS_ORDERED_LIST),
        );
        let result = store.query_to_json(&sparql_lists)?;
        let rows = result.as_array().expect("expected array");
        assert!(
            rows.len() >= 2,
            "expected at least 2 lists, got {}",
            rows.len()
        );

        // Should have equations
        let sparql_eq = format!(
            "SELECT ?f WHERE {{ \
               GRAPH <{doc_graph}> {{ \
                 ?f a <{}> \
               }} \
             }}",
            ont::iri(ont::CLASS_FORMULA),
        );
        let result = store.query_to_json(&sparql_eq)?;
        let rows = result.as_array().expect("expected array");
        assert!(
            rows.len() >= 3,
            "expected at least 3 formulas, got {}",
            rows.len()
        );

        // Should have code blocks
        let sparql_code = format!(
            "SELECT ?c WHERE {{ \
               GRAPH <{doc_graph}> {{ \
                 ?c a <{}> \
               }} \
             }}",
            ont::iri(ont::CLASS_CODE),
        );
        let result = store.query_to_json(&sparql_code)?;
        let rows = result.as_array().expect("expected array");
        assert!(
            rows.len() >= 2,
            "expected at least 2 code blocks, got {}",
            rows.len()
        );

        // Should have at least one footnote
        let sparql_fn = format!(
            "SELECT ?f WHERE {{ \
               GRAPH <{doc_graph}> {{ \
                 ?f a <{}> \
               }} \
             }}",
            ont::iri(ont::CLASS_FOOTNOTE),
        );
        let result = store.query_to_json(&sparql_fn)?;
        let rows = result.as_array().expect("expected array");
        assert!(
            rows.len() >= 1,
            "expected at least 1 footnote, got {}",
            rows.len()
        );

        // Triple count should be substantial
        let count = store.triple_count_in(&doc_graph)?;
        assert!(count > 50, "expected >50 triples, got {count}");

        Ok(())
    }

    // -------------------------------------------------------------------
    // Tokenizer unit tests
    // -------------------------------------------------------------------

    #[test]
    fn tokenize_basic_command() {
        let tokens = tokenize(r"\section{Hello}");
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0], Token::Command("section".to_string()));
        assert_eq!(tokens[1], Token::OpenBrace);
        assert_eq!(tokens[2], Token::Text("Hello".to_string()));
        assert_eq!(tokens[3], Token::CloseBrace);
    }

    #[test]
    fn tokenize_begin_end() {
        let tokens = tokenize(r"\begin{itemize}\end{itemize}");
        assert_eq!(tokens[0], Token::BeginEnv("itemize".to_string()));
        assert_eq!(tokens[1], Token::EndEnv("itemize".to_string()));
    }

    #[test]
    fn tokenize_display_math() {
        let tokens = tokenize(r"$$E=mc^2$$");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0], Token::DisplayMath("E=mc^2".to_string()));
    }

    #[test]
    fn tokenize_bracket_math() {
        let tokens = tokenize(r"\[x^2\]");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0], Token::BracketMath("x^2".to_string()));
    }

    #[test]
    fn tokenize_strips_comments() {
        let tokens = tokenize("hello % comment\nworld");
        // "hello " is text, then comment is stripped, "world" is text on next line
        // After the comment, the newline is consumed by the comment-skip loop
        assert!(tokens.len() >= 1);
        let combined: String = tokens
            .iter()
            .filter_map(|t| match t {
                Token::Text(s) => Some(s.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");
        assert!(combined.contains("hello"));
        assert!(combined.contains("world"));
        assert!(!combined.contains("comment"));
    }

    // -------------------------------------------------------------------
    // Empty document
    // -------------------------------------------------------------------

    #[test]
    fn parse_empty_document() -> ruddydoc_core::Result<()> {
        let tex = "";
        let (store, meta, graph) = parse_latex(tex)?;

        assert_eq!(meta.format, InputFormat::Latex);

        // Should still have the document node
        let doc_iri = ruddydoc_core::doc_iri(&meta.hash.0);
        let sparql = format!(
            "ASK {{ GRAPH <{graph}> {{ <{doc_iri}> a <{}> }} }}",
            ont::iri(ont::CLASS_DOCUMENT),
        );
        let result = store.query_to_json(&sparql)?;
        assert_eq!(result, serde_json::Value::Bool(true));
        Ok(())
    }
}
