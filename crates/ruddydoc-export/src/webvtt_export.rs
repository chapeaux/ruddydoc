//! WebVTT exporter: produce Web Video Text Tracks output.
//!
//! For documents with timing information (`rdoc:startTime` and `rdoc:endTime`),
//! emits standard WebVTT cues. For documents without timing data, produces
//! sequential pseudo-timestamps based on reading order.

use ruddydoc_core::{DocumentExporter, DocumentStore, OutputFormat};
use ruddydoc_ontology as ont;

/// WebVTT exporter.
pub struct WebVttExporter;

impl DocumentExporter for WebVttExporter {
    fn format(&self) -> OutputFormat {
        OutputFormat::WebVtt
    }

    fn export(&self, store: &dyn DocumentStore, doc_graph: &str) -> ruddydoc_core::Result<String> {
        let mut output = String::from("WEBVTT\n");

        // First, try to get timed elements (those with startTime and endTime)
        let timed = query_timed_elements(store, doc_graph)?;

        if !timed.is_empty() {
            for cue in &timed {
                output.push('\n');
                output.push_str(&format!(
                    "{} --> {}\n",
                    format_timestamp(&cue.start_time),
                    format_timestamp(&cue.end_time),
                ));
                output.push_str(&cue.text);
                output.push('\n');
            }
        } else {
            // No timing info: generate pseudo-timestamps from text elements
            let texts = query_text_elements(store, doc_graph)?;
            let duration_per_cue = 5.0_f64; // seconds per cue

            for (i, text) in texts.iter().enumerate() {
                if text.is_empty() {
                    continue;
                }
                let start_secs = i as f64 * duration_per_cue;
                let end_secs = start_secs + duration_per_cue;
                output.push('\n');
                output.push_str(&format!(
                    "{} --> {}\n",
                    format_seconds(start_secs),
                    format_seconds(end_secs),
                ));
                output.push_str(text);
                output.push('\n');
            }
        }

        Ok(output)
    }
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

struct TimedCue {
    start_time: String,
    end_time: String,
    text: String,
}

// ---------------------------------------------------------------------------
// Query helpers
// ---------------------------------------------------------------------------

/// Extract a clean string from a SPARQL literal result.
fn clean_literal(s: &str) -> String {
    if let Some(idx) = s.find("\"^^<") {
        return s[1..idx].to_string();
    }
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        return s[1..s.len() - 1].to_string();
    }
    s.to_string()
}

/// Query elements that have timing information.
fn query_timed_elements(
    store: &dyn DocumentStore,
    doc_graph: &str,
) -> ruddydoc_core::Result<Vec<TimedCue>> {
    let sparql = format!(
        "SELECT ?text ?start ?end WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             ?el <{text_content}> ?text. \
             ?el <{start_time}> ?start. \
             ?el <{end_time}> ?end. \
             ?el <{reading_order}> ?order \
           }} \
         }} ORDER BY ?order",
        text_content = ont::iri(ont::PROP_TEXT_CONTENT),
        start_time = ont::iri(ont::PROP_START_TIME),
        end_time = ont::iri(ont::PROP_END_TIME),
        reading_order = ont::iri(ont::PROP_READING_ORDER),
    );
    let result = store.query_to_json(&sparql)?;
    let mut cues = Vec::new();

    if let Some(rows) = result.as_array() {
        for row in rows {
            let text = row
                .get("text")
                .and_then(|v| v.as_str())
                .map(clean_literal)
                .unwrap_or_default();
            let start = row
                .get("start")
                .and_then(|v| v.as_str())
                .map(clean_literal)
                .unwrap_or_default();
            let end = row
                .get("end")
                .and_then(|v| v.as_str())
                .map(clean_literal)
                .unwrap_or_default();

            if !text.is_empty() {
                cues.push(TimedCue {
                    start_time: start,
                    end_time: end,
                    text,
                });
            }
        }
    }

    Ok(cues)
}

/// Query all text elements in reading order (for pseudo-timestamp fallback).
fn query_text_elements(
    store: &dyn DocumentStore,
    doc_graph: &str,
) -> ruddydoc_core::Result<Vec<String>> {
    let sparql = format!(
        "SELECT ?text WHERE {{ \
           GRAPH <{doc_graph}> {{ \
             ?el <{text_content}> ?text. \
             ?el <{reading_order}> ?order. \
             ?el a ?type. \
             FILTER(?type IN ( \
               <{section_header}>, \
               <{paragraph}>, \
               <{list_item}>, \
               <{code}>, \
               <{title}> \
             )) \
           }} \
         }} ORDER BY ?order",
        text_content = ont::iri(ont::PROP_TEXT_CONTENT),
        reading_order = ont::iri(ont::PROP_READING_ORDER),
        section_header = ont::iri(ont::CLASS_SECTION_HEADER),
        paragraph = ont::iri(ont::CLASS_PARAGRAPH),
        list_item = ont::iri(ont::CLASS_LIST_ITEM),
        code = ont::iri(ont::CLASS_CODE),
        title = ont::iri(ont::CLASS_TITLE),
    );
    let result = store.query_to_json(&sparql)?;
    let mut texts = Vec::new();

    if let Some(rows) = result.as_array() {
        for row in rows {
            let text = row
                .get("text")
                .and_then(|v| v.as_str())
                .map(clean_literal)
                .unwrap_or_default();
            if !text.is_empty() {
                texts.push(text);
            }
        }
    }

    Ok(texts)
}

// ---------------------------------------------------------------------------
// Timestamp formatting
// ---------------------------------------------------------------------------

/// Format an xsd:duration or raw time string into WebVTT timestamp format.
///
/// Handles ISO 8601 duration format (e.g., "PT1H2M3.4S") or pre-formatted
/// timestamp strings (e.g., "00:01:02.000"). If the input is already in
/// HH:MM:SS.mmm format, it is returned as-is.
fn format_timestamp(duration: &str) -> String {
    // If already in WebVTT format (contains colons), return as-is
    if duration.contains(':') {
        return duration.to_string();
    }

    // Try to parse ISO 8601 duration: PT[nH][nM][n.nS]
    if let Some(rest) = duration.strip_prefix("PT") {
        let total_seconds = parse_iso_duration(rest);
        return format_seconds(total_seconds);
    }

    // Fallback: try parsing as plain seconds
    if let Ok(secs) = duration.parse::<f64>() {
        return format_seconds(secs);
    }

    // Last resort: return the original string
    duration.to_string()
}

/// Parse an ISO 8601 duration body (after "PT") into total seconds.
fn parse_iso_duration(body: &str) -> f64 {
    let mut total: f64 = 0.0;
    let mut current = String::new();

    for ch in body.chars() {
        match ch {
            'H' | 'h' => {
                if let Ok(hours) = current.parse::<f64>() {
                    total += hours * 3600.0;
                }
                current.clear();
            }
            'M' | 'm' => {
                if let Ok(minutes) = current.parse::<f64>() {
                    total += minutes * 60.0;
                }
                current.clear();
            }
            'S' | 's' => {
                if let Ok(seconds) = current.parse::<f64>() {
                    total += seconds;
                }
                current.clear();
            }
            _ => {
                current.push(ch);
            }
        }
    }

    total
}

/// Format a number of seconds into WebVTT timestamp (HH:MM:SS.mmm).
fn format_seconds(total_secs: f64) -> String {
    let total_ms = (total_secs * 1000.0).round() as u64;
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let seconds = (total_ms % 60_000) / 1_000;
    let millis = total_ms % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
}

#[cfg(test)]
mod internal_tests {
    use super::*;

    #[test]
    fn format_seconds_basic() {
        assert_eq!(format_seconds(0.0), "00:00:00.000");
        assert_eq!(format_seconds(1.0), "00:00:01.000");
        assert_eq!(format_seconds(61.5), "00:01:01.500");
        assert_eq!(format_seconds(3661.123), "01:01:01.123");
    }

    #[test]
    fn format_timestamp_passthrough() {
        assert_eq!(format_timestamp("00:01:02.000"), "00:01:02.000");
    }

    #[test]
    fn format_timestamp_iso_duration() {
        assert_eq!(format_timestamp("PT1H2M3S"), "01:02:03.000");
        assert_eq!(format_timestamp("PT5.5S"), "00:00:05.500");
        assert_eq!(format_timestamp("PT1M"), "00:01:00.000");
    }

    #[test]
    fn parse_iso_duration_parts() {
        assert!((parse_iso_duration("1H") - 3600.0).abs() < f64::EPSILON);
        assert!((parse_iso_duration("30M") - 1800.0).abs() < f64::EPSILON);
        assert!((parse_iso_duration("10S") - 10.0).abs() < f64::EPSILON);
        assert!((parse_iso_duration("1H30M15S") - 5415.0).abs() < f64::EPSILON);
    }
}
