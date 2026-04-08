//! Shared fixture generators for RuddyDoc benchmarks.
//!
//! Produces realistic, deterministic Markdown and HTML content at various
//! scales for measuring parsing and export performance.

use sha2::{Digest, Sha256};

/// Compute a SHA-256 hex-encoded hash of the given bytes.
pub fn compute_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    result.iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Generate a realistic Markdown document with approximately `n_lines` lines.
///
/// The document includes headings, paragraphs, bullet lists, ordered lists,
/// code blocks, tables, and images -- distributed in realistic proportions.
pub fn generate_large_markdown(n_lines: usize) -> String {
    let mut out = String::with_capacity(n_lines * 60);
    let mut line = 0;

    // Title
    out.push_str("# Benchmark Document: Comprehensive Analysis\n\n");
    line += 2;

    let mut section = 0;
    while line < n_lines {
        section += 1;

        // Section heading
        out.push_str(&format!(
            "## Section {section}: Analysis of Topic {section}\n\n"
        ));
        line += 2;
        if line >= n_lines {
            break;
        }

        // Introductory paragraph
        out.push_str(&format!(
            "This section explores topic {section} in depth. The analysis covers \
             multiple dimensions including quantitative metrics, qualitative \
             assessments, and comparative evaluations against baseline measurements. \
             Understanding these factors is essential for drawing meaningful \
             conclusions about the underlying patterns.\n\n"
        ));
        line += 2;
        if line >= n_lines {
            break;
        }

        // Another paragraph with different content
        out.push_str(&format!(
            "The data collected for section {section} spans a period of twelve \
             months, encompassing seasonal variations and external influences. \
             Statistical significance was established at the p < 0.05 level using \
             a two-tailed t-test with Bonferroni correction for multiple comparisons.\n\n"
        ));
        line += 2;
        if line >= n_lines {
            break;
        }

        // Subsection with bullet list
        out.push_str(&format!("### Key Findings for Topic {section}\n\n"));
        line += 2;
        if line >= n_lines {
            break;
        }

        for i in 1..=5 {
            out.push_str(&format!(
                "- Finding {section}.{i}: The measured value exceeded the expected \
                 threshold by {val:.1}%, indicating a statistically significant \
                 deviation from the null hypothesis\n",
                val = (section as f64 * 3.7 + i as f64 * 2.1) % 47.0 + 5.0
            ));
            line += 1;
            if line >= n_lines {
                break;
            }
        }
        out.push('\n');
        line += 1;
        if line >= n_lines {
            break;
        }

        // Subsection with ordered list
        out.push_str(&format!("### Methodology for Topic {section}\n\n"));
        line += 2;
        if line >= n_lines {
            break;
        }

        for i in 1..=4 {
            out.push_str(&format!(
                "{i}. Prepare sample batch {section}-{i} using the standardized protocol \
                 and verify calibration settings against reference standards\n"
            ));
            line += 1;
            if line >= n_lines {
                break;
            }
        }
        out.push('\n');
        line += 1;
        if line >= n_lines {
            break;
        }

        // Code block
        out.push_str("```python\n");
        out.push_str(&format!("def analyze_topic_{section}(data):\n"));
        out.push_str(&format!("    \"\"\"Analyze topic {section} data.\"\"\"\n"));
        out.push_str("    results = []\n");
        out.push_str("    for sample in data:\n");
        out.push_str(&format!(
            "        value = sample.measure() * {factor:.2}\n",
            factor = 1.0 + section as f64 * 0.15
        ));
        out.push_str("        results.append(value)\n");
        out.push_str("    return statistics.mean(results)\n");
        out.push_str("```\n\n");
        line += 10;
        if line >= n_lines {
            break;
        }

        // Table
        out.push_str(&format!("### Results Table {section}\n\n"));
        out.push_str("| Metric | Baseline | Measured | Delta | Status |\n");
        out.push_str("|--------|----------|----------|-------|--------|\n");
        line += 4;
        for row in 1..=4 {
            let baseline = 50.0 + (section as f64 * 7.3 + row as f64 * 4.1) % 40.0;
            let measured = baseline + (section as f64 * 1.7 + row as f64 * 2.9) % 15.0 - 5.0;
            let delta = measured - baseline;
            let status = if delta > 0.0 { "Improved" } else { "Declined" };
            out.push_str(&format!(
                "| Metric {section}.{row} | {baseline:.1} | {measured:.1} | {delta:+.1} | {status} |\n"
            ));
            line += 1;
            if line >= n_lines {
                break;
            }
        }
        out.push('\n');
        line += 1;
        if line >= n_lines {
            break;
        }

        // Image reference
        out.push_str(&format!(
            "![Figure {section}: Visualization of topic {section} results](figures/topic_{section}.png)\n\n"
        ));
        line += 2;
        if line >= n_lines {
            break;
        }

        // Block quote
        out.push_str(&format!(
            "> The results for topic {section} demonstrate a clear trend toward \
             convergence, suggesting that the system reaches steady state under \
             the tested conditions. -- Research Team Report, 2025\n\n"
        ));
        line += 2;
    }

    // Conclusion
    out.push_str("## Conclusion\n\n");
    out.push_str(
        "This document has presented a comprehensive analysis across all topics. \
         The findings support the primary hypothesis and provide a foundation \
         for future research directions.\n",
    );

    out
}

/// Generate a realistic HTML document with approximately `n_elements` structural elements.
pub fn generate_large_html(n_elements: usize) -> String {
    let mut out = String::with_capacity(n_elements * 200);
    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
    out.push_str("  <meta charset=\"UTF-8\">\n");
    out.push_str("  <meta name=\"description\" content=\"Benchmark HTML document\">\n");
    out.push_str("  <title>Benchmark HTML Document</title>\n");
    out.push_str("</head>\n<body>\n");
    out.push_str("  <header><h1>Benchmark HTML Document</h1></header>\n");
    out.push_str("  <main>\n");

    let mut el = 0;
    let mut section = 0;

    while el < n_elements {
        section += 1;

        // Section heading
        out.push_str(&format!("    <h2>Section {section}</h2>\n"));
        el += 1;
        if el >= n_elements {
            break;
        }

        // Paragraphs
        for p in 1..=3 {
            out.push_str(&format!(
                "    <p>Paragraph {p} of section {section}. This contains a mix of \
                 <strong>bold</strong>, <em>italic</em>, and <code>inline code</code> \
                 elements to exercise the HTML parser thoroughly. The text is designed \
                 to be realistic and representative of actual web content.</p>\n"
            ));
            el += 1;
            if el >= n_elements {
                break;
            }
        }
        if el >= n_elements {
            break;
        }

        // Unordered list
        out.push_str("    <ul>\n");
        for i in 1..=4 {
            out.push_str(&format!(
                "      <li>Item {section}.{i}: Description of this list entry</li>\n"
            ));
            el += 1;
            if el >= n_elements {
                break;
            }
        }
        out.push_str("    </ul>\n");
        if el >= n_elements {
            break;
        }

        // Table
        out.push_str("    <table>\n      <thead>\n        <tr>\n");
        out.push_str("          <th>Column A</th><th>Column B</th><th>Column C</th>\n");
        out.push_str("        </tr>\n      </thead>\n      <tbody>\n");
        el += 1;
        for row in 1..=3 {
            out.push_str("        <tr>\n");
            for col in 1..=3 {
                out.push_str(&format!("          <td>Cell {section}-{row}-{col}</td>\n"));
            }
            out.push_str("        </tr>\n");
            el += 1;
            if el >= n_elements {
                break;
            }
        }
        out.push_str("      </tbody>\n    </table>\n");
        if el >= n_elements {
            break;
        }

        // Code block
        out.push_str("    <pre><code>function process(data) {\n");
        out.push_str(&format!("  // Section {section} processing\n"));
        out.push_str("  return data.map(x =&gt; x * 2);\n");
        out.push_str("}</code></pre>\n");
        el += 1;
    }

    out.push_str("  </main>\n");
    out.push_str("  <footer><p>End of benchmark document.</p></footer>\n");
    out.push_str("</body>\n</html>\n");

    out
}

/// Generate a realistic CSV document with `n_rows` data rows plus a header.
pub fn generate_large_csv(n_rows: usize) -> String {
    let mut out = String::with_capacity(n_rows * 80);
    out.push_str("Name,Department,Revenue,Costs,Profit,Region,Quarter\n");

    let names = [
        "Alpha Corp",
        "Beta Industries",
        "Gamma Solutions",
        "Delta Systems",
        "Epsilon Analytics",
        "Zeta Dynamics",
        "Eta Consulting",
        "Theta Networks",
    ];
    let departments = ["Engineering", "Sales", "Marketing", "Support", "Research"];
    let regions = ["North", "South", "East", "West", "Central"];

    for i in 0..n_rows {
        let name = names[i % names.len()];
        let dept = departments[i % departments.len()];
        let region = regions[i % regions.len()];
        let revenue = 50000 + (i * 1731) % 200000;
        let costs = 30000 + (i * 1117) % 120000;
        let profit = revenue as i64 - costs as i64;
        let quarter = format!("Q{}", (i % 4) + 1);
        out.push_str(&format!(
            "{name},{dept},{revenue},{costs},{profit},{region},{quarter}\n"
        ));
    }

    out
}

/// Generate a realistic LaTeX document with approximately `n_lines` lines.
pub fn generate_large_latex(n_lines: usize) -> String {
    let mut out = String::with_capacity(n_lines * 60);
    out.push_str("\\documentclass{article}\n");
    out.push_str("\\usepackage{amsmath}\n");
    out.push_str("\\usepackage{graphicx}\n\n");
    out.push_str("\\title{Benchmark LaTeX Document}\n");
    out.push_str("\\author{RuddyDoc Benchmark Suite}\n");
    out.push_str("\\date{2026}\n\n");
    out.push_str("\\begin{document}\n\n");
    out.push_str("\\maketitle\n\n");

    let mut line = 12;
    let mut section = 0;

    while line < n_lines {
        section += 1;

        out.push_str(&format!("\\section{{Analysis of Dataset {section}}}\n\n"));
        line += 2;
        if line >= n_lines {
            break;
        }

        out.push_str(&format!(
            "This section presents the analysis of dataset {section}. The data was \
             collected over a twelve-month period and processed using standard \
             statistical methods including regression analysis and hypothesis testing.\n\n"
        ));
        line += 2;
        if line >= n_lines {
            break;
        }

        out.push_str(&format!(
            "\\subsection{{Methodology for Dataset {section}}}\n\n"
        ));
        line += 2;
        if line >= n_lines {
            break;
        }

        out.push_str("\\begin{itemize}\n");
        for i in 1..=4 {
            out.push_str(&format!(
                "    \\item Step {i}: Execute protocol {section}-{i} under controlled conditions\n"
            ));
            line += 1;
            if line >= n_lines {
                break;
            }
        }
        out.push_str("\\end{itemize}\n\n");
        line += 2;
        if line >= n_lines {
            break;
        }

        // Table
        out.push_str("\\begin{table}[h]\n\\centering\n");
        out.push_str("\\begin{tabular}{|l|r|r|r|}\n\\hline\n");
        out.push_str("\\textbf{Metric} & \\textbf{Baseline} & \\textbf{Measured} & \\textbf{Delta} \\\\\n\\hline\n");
        line += 5;
        for row in 1..=3 {
            let baseline = 42.0 + (section as f64 * 5.3 + row as f64 * 3.7) % 30.0;
            let measured = baseline + (section as f64 * 1.1 + row as f64 * 2.3) % 10.0 - 3.0;
            let delta = measured - baseline;
            out.push_str(&format!(
                "Metric {section}.{row} & {baseline:.1} & {measured:.1} & {delta:+.1} \\\\\n"
            ));
            line += 1;
            if line >= n_lines {
                break;
            }
        }
        out.push_str("\\hline\n\\end{tabular}\n");
        out.push_str(&format!("\\caption{{Results for dataset {section}}}\n"));
        out.push_str(&format!("\\label{{tab:dataset{section}}}\n"));
        out.push_str("\\end{table}\n\n");
        line += 5;
        if line >= n_lines {
            break;
        }

        // Equation
        out.push_str("\\begin{equation}\n");
        out.push_str(&format!(
            "    \\bar{{x}}_{section} = \\frac{{1}}{{n}} \\sum_{{i=1}}^{{n}} x_i\n"
        ));
        out.push_str(&format!("\\label{{eq:mean{section}}}\n"));
        out.push_str("\\end{equation}\n\n");
        line += 5;
        if line >= n_lines {
            break;
        }

        // Code block
        out.push_str("\\begin{verbatim}\n");
        out.push_str(&format!("def process_dataset_{section}(data):\n"));
        out.push_str(&format!(
            "    return [x * {factor:.2} for x in data]\n",
            factor = 1.0 + section as f64 * 0.1
        ));
        out.push_str("\\end{verbatim}\n\n");
        line += 5;
    }

    out.push_str("\\section{Conclusion}\n\n");
    out.push_str("This document has covered all benchmark datasets. The results confirm the primary hypothesis.\n\n");
    out.push_str("\\end{document}\n");

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_markdown_100_lines() {
        let md = generate_large_markdown(100);
        let line_count = md.lines().count();
        assert!(
            line_count >= 80,
            "expected at least 80 lines, got {line_count}"
        );
        assert!(md.contains("# Benchmark Document"));
        assert!(md.contains("## Section 1"));
        assert!(md.contains("| Metric"));
        assert!(md.contains("```python"));
    }

    #[test]
    fn generate_markdown_1000_lines() {
        let md = generate_large_markdown(1000);
        let line_count = md.lines().count();
        assert!(
            line_count >= 800,
            "expected at least 800 lines, got {line_count}"
        );
    }

    #[test]
    fn generate_html_500_elements() {
        let html = generate_large_html(500);
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<h2>Section 1</h2>"));
        assert!(html.contains("<table>"));
        assert!(html.contains("<ul>"));
        assert!(html.contains("</html>"));
    }

    #[test]
    fn generate_csv_100_rows() {
        let csv_str = generate_large_csv(100);
        let line_count = csv_str.lines().count();
        // header + 100 data rows
        assert_eq!(line_count, 101);
    }

    #[test]
    fn generate_latex_200_lines() {
        let tex = generate_large_latex(200);
        assert!(tex.contains("\\documentclass{article}"));
        assert!(tex.contains("\\section{"));
        assert!(tex.contains("\\begin{table}"));
        assert!(tex.contains("\\end{document}"));
    }

    #[test]
    fn compute_hash_deterministic() {
        let h1 = compute_hash(b"hello");
        let h2 = compute_hash(b"hello");
        assert_eq!(h1, h2);
        assert_ne!(h1, compute_hash(b"world"));
    }
}
