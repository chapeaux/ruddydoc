//! Post-processing utilities for ML model outputs.
//!
//! Includes non-maximum suppression, IoU computation, confidence filtering,
//! reading order sorting, and ontology class mapping.

use ruddydoc_core::BoundingBox;

use crate::types::{DetectedRegion, RegionLabel};

/// Compute Intersection over Union (IoU) between two bounding boxes.
///
/// Returns a value in `[0.0, 1.0]`. Returns `0.0` if either box has
/// zero area or the boxes do not overlap.
pub fn iou(a: &BoundingBox, b: &BoundingBox) -> f32 {
    let inter_left = a.left.max(b.left);
    let inter_top = a.top.max(b.top);
    let inter_right = a.right.min(b.right);
    let inter_bottom = a.bottom.min(b.bottom);

    let inter_width = (inter_right - inter_left).max(0.0);
    let inter_height = (inter_bottom - inter_top).max(0.0);
    let inter_area = inter_width * inter_height;

    let area_a = (a.right - a.left).max(0.0) * (a.bottom - a.top).max(0.0);
    let area_b = (b.right - b.left).max(0.0) * (b.bottom - b.top).max(0.0);
    let union_area = area_a + area_b - inter_area;

    if union_area <= 0.0 {
        0.0
    } else {
        (inter_area / union_area) as f32
    }
}

/// Apply non-maximum suppression to a list of detected regions.
///
/// Regions are sorted by confidence (descending), then for each region,
/// any subsequent region with IoU exceeding `iou_threshold` is removed.
///
/// The vector is modified in-place: suppressed regions are removed.
pub fn nms(regions: &mut Vec<DetectedRegion>, iou_threshold: f32) {
    // Sort by confidence descending.
    regions.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut keep = vec![true; regions.len()];

    for i in 0..regions.len() {
        if !keep[i] {
            continue;
        }
        for j in (i + 1)..regions.len() {
            if !keep[j] {
                continue;
            }
            if iou(&regions[i].bbox, &regions[j].bbox) > iou_threshold {
                keep[j] = false;
            }
        }
    }

    let mut idx = 0;
    regions.retain(|_| {
        let kept = keep[idx];
        idx += 1;
        kept
    });
}

/// Filter detected regions by a minimum confidence threshold.
///
/// Regions with confidence strictly below `threshold` are removed.
pub fn filter_by_confidence(regions: &mut Vec<DetectedRegion>, threshold: f32) {
    regions.retain(|r| r.confidence >= threshold);
}

/// Sort detected regions into reading order: top-to-bottom, then
/// left-to-right within the same vertical band.
///
/// Two regions are considered to be on the same vertical line if their
/// vertical midpoints are within 20% of the average region height.
pub fn reading_order_sort(regions: &mut [DetectedRegion]) {
    if regions.is_empty() {
        return;
    }

    // Compute average height for vertical band tolerance.
    let avg_height: f64 = regions
        .iter()
        .map(|r| (r.bbox.bottom - r.bbox.top).abs())
        .sum::<f64>()
        / regions.len() as f64;
    let tolerance = avg_height * 0.2;

    regions.sort_by(|a, b| {
        let mid_a = (a.bbox.top + a.bbox.bottom) / 2.0;
        let mid_b = (b.bbox.top + b.bbox.bottom) / 2.0;

        if (mid_a - mid_b).abs() < tolerance {
            // Same vertical band: sort by left edge.
            a.bbox
                .left
                .partial_cmp(&b.bbox.left)
                .unwrap_or(std::cmp::Ordering::Equal)
        } else {
            // Different vertical bands: sort by top edge.
            a.bbox
                .top
                .partial_cmp(&b.bbox.top)
                .unwrap_or(std::cmp::Ordering::Equal)
        }
    });
}

/// Map a `RegionLabel` to its corresponding RuddyDoc ontology class name.
///
/// These map to `rdoc:` namespace classes defined in the document ontology.
pub fn label_to_ontology_class(label: RegionLabel) -> &'static str {
    match label {
        RegionLabel::Title => "Title",
        RegionLabel::SectionHeader => "SectionHeader",
        RegionLabel::Paragraph => "TextElement",
        RegionLabel::List => "ListElement",
        RegionLabel::Table => "Table",
        RegionLabel::Picture => "Picture",
        RegionLabel::Caption => "Caption",
        RegionLabel::Footnote => "Footnote",
        RegionLabel::PageHeader => "PageHeader",
        RegionLabel::PageFooter => "PageFooter",
        RegionLabel::Formula => "Formula",
        RegionLabel::Code => "CodeBlock",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn bbox(left: f64, top: f64, right: f64, bottom: f64) -> BoundingBox {
        BoundingBox {
            left,
            top,
            right,
            bottom,
        }
    }

    fn region(
        label: RegionLabel,
        left: f64,
        top: f64,
        right: f64,
        bottom: f64,
        confidence: f32,
    ) -> DetectedRegion {
        DetectedRegion {
            label,
            bbox: bbox(left, top, right, bottom),
            confidence,
        }
    }

    // --- IoU tests ---

    #[test]
    fn iou_identical_boxes() {
        let a = bbox(0.0, 0.0, 10.0, 10.0);
        let b = bbox(0.0, 0.0, 10.0, 10.0);
        assert!((iou(&a, &b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn iou_no_overlap() {
        let a = bbox(0.0, 0.0, 10.0, 10.0);
        let b = bbox(20.0, 20.0, 30.0, 30.0);
        assert!((iou(&a, &b)).abs() < 1e-6);
    }

    #[test]
    fn iou_partial_overlap() {
        let a = bbox(0.0, 0.0, 10.0, 10.0); // area = 100
        let b = bbox(5.0, 5.0, 15.0, 15.0); // area = 100
        // Intersection: (5,5)-(10,10) = 5*5 = 25
        // Union: 100 + 100 - 25 = 175
        // IoU = 25/175 = 0.142857...
        let result = iou(&a, &b);
        assert!((result - 25.0 / 175.0).abs() < 1e-5);
    }

    #[test]
    fn iou_containment() {
        let a = bbox(0.0, 0.0, 20.0, 20.0); // area = 400
        let b = bbox(5.0, 5.0, 15.0, 15.0); // area = 100
        // Intersection = 100
        // Union = 400 + 100 - 100 = 400
        // IoU = 100/400 = 0.25
        let result = iou(&a, &b);
        assert!((result - 0.25).abs() < 1e-5);
    }

    #[test]
    fn iou_touching_edge() {
        let a = bbox(0.0, 0.0, 10.0, 10.0);
        let b = bbox(10.0, 0.0, 20.0, 10.0);
        // Touching at edge: intersection width = 0
        assert!((iou(&a, &b)).abs() < 1e-6);
    }

    #[test]
    fn iou_zero_area_box() {
        let a = bbox(0.0, 0.0, 0.0, 10.0); // zero width
        let b = bbox(0.0, 0.0, 10.0, 10.0);
        assert!((iou(&a, &b)).abs() < 1e-6);
    }

    // --- NMS tests ---

    #[test]
    fn nms_no_overlap() {
        let mut regions = vec![
            region(RegionLabel::Paragraph, 0.0, 0.0, 10.0, 10.0, 0.9),
            region(RegionLabel::Paragraph, 50.0, 50.0, 60.0, 60.0, 0.8),
        ];
        nms(&mut regions, 0.5);
        assert_eq!(regions.len(), 2);
    }

    #[test]
    fn nms_full_overlap_removes_lower_confidence() {
        let mut regions = vec![
            region(RegionLabel::Paragraph, 0.0, 0.0, 10.0, 10.0, 0.9),
            region(RegionLabel::Paragraph, 0.0, 0.0, 10.0, 10.0, 0.7),
        ];
        nms(&mut regions, 0.5);
        assert_eq!(regions.len(), 1);
        assert!((regions[0].confidence - 0.9).abs() < 1e-6);
    }

    #[test]
    fn nms_partial_overlap_below_threshold() {
        let mut regions = vec![
            region(RegionLabel::Paragraph, 0.0, 0.0, 10.0, 10.0, 0.9),
            region(RegionLabel::Paragraph, 8.0, 8.0, 18.0, 18.0, 0.8),
        ];
        // IoU = 4/196 = 0.0204... which is below 0.5 threshold
        nms(&mut regions, 0.5);
        assert_eq!(regions.len(), 2);
    }

    #[test]
    fn nms_empty_list() {
        let mut regions: Vec<DetectedRegion> = vec![];
        nms(&mut regions, 0.5);
        assert!(regions.is_empty());
    }

    #[test]
    fn nms_three_overlapping() {
        let mut regions = vec![
            region(RegionLabel::Title, 0.0, 0.0, 10.0, 10.0, 0.6),
            region(RegionLabel::Title, 0.0, 0.0, 10.0, 10.0, 0.9),
            region(RegionLabel::Title, 0.0, 0.0, 10.0, 10.0, 0.7),
        ];
        nms(&mut regions, 0.5);
        assert_eq!(regions.len(), 1);
        assert!((regions[0].confidence - 0.9).abs() < 1e-6);
    }

    // --- Confidence filtering tests ---

    #[test]
    fn filter_by_confidence_basic() {
        let mut regions = vec![
            region(RegionLabel::Paragraph, 0.0, 0.0, 10.0, 10.0, 0.9),
            region(RegionLabel::Paragraph, 0.0, 0.0, 10.0, 10.0, 0.3),
            region(RegionLabel::Paragraph, 0.0, 0.0, 10.0, 10.0, 0.5),
        ];
        filter_by_confidence(&mut regions, 0.5);
        assert_eq!(regions.len(), 2);
        assert!(regions.iter().all(|r| r.confidence >= 0.5));
    }

    #[test]
    fn filter_by_confidence_none_pass() {
        let mut regions = vec![
            region(RegionLabel::Paragraph, 0.0, 0.0, 10.0, 10.0, 0.1),
            region(RegionLabel::Paragraph, 0.0, 0.0, 10.0, 10.0, 0.2),
        ];
        filter_by_confidence(&mut regions, 0.5);
        assert!(regions.is_empty());
    }

    #[test]
    fn filter_by_confidence_all_pass() {
        let mut regions = vec![
            region(RegionLabel::Paragraph, 0.0, 0.0, 10.0, 10.0, 0.8),
            region(RegionLabel::Paragraph, 0.0, 0.0, 10.0, 10.0, 0.9),
        ];
        filter_by_confidence(&mut regions, 0.5);
        assert_eq!(regions.len(), 2);
    }

    // --- Reading order tests ---

    #[test]
    fn reading_order_top_to_bottom() {
        let mut regions = vec![
            region(RegionLabel::Paragraph, 0.0, 100.0, 50.0, 120.0, 0.9),
            region(RegionLabel::Paragraph, 0.0, 0.0, 50.0, 20.0, 0.9),
            region(RegionLabel::Paragraph, 0.0, 50.0, 50.0, 70.0, 0.9),
        ];
        reading_order_sort(&mut regions);
        // Should be ordered: top=0, top=50, top=100
        assert!((regions[0].bbox.top).abs() < 1e-6);
        assert!((regions[1].bbox.top - 50.0).abs() < 1e-6);
        assert!((regions[2].bbox.top - 100.0).abs() < 1e-6);
    }

    #[test]
    fn reading_order_left_to_right_same_line() {
        // Three regions on the same vertical line (similar tops)
        let mut regions = vec![
            region(RegionLabel::Paragraph, 200.0, 10.0, 300.0, 30.0, 0.9),
            region(RegionLabel::Paragraph, 0.0, 10.0, 100.0, 30.0, 0.9),
            region(RegionLabel::Paragraph, 100.0, 10.0, 200.0, 30.0, 0.9),
        ];
        reading_order_sort(&mut regions);
        assert!((regions[0].bbox.left).abs() < 1e-6);
        assert!((regions[1].bbox.left - 100.0).abs() < 1e-6);
        assert!((regions[2].bbox.left - 200.0).abs() < 1e-6);
    }

    #[test]
    fn reading_order_empty() {
        let mut regions: Vec<DetectedRegion> = vec![];
        reading_order_sort(&mut regions);
        assert!(regions.is_empty());
    }

    #[test]
    fn reading_order_single() {
        let mut regions = vec![region(RegionLabel::Title, 10.0, 20.0, 100.0, 40.0, 0.9)];
        reading_order_sort(&mut regions);
        assert_eq!(regions.len(), 1);
    }

    // --- Ontology class mapping tests ---

    #[test]
    fn label_to_ontology_all_labels() {
        assert_eq!(label_to_ontology_class(RegionLabel::Title), "Title");
        assert_eq!(
            label_to_ontology_class(RegionLabel::SectionHeader),
            "SectionHeader"
        );
        assert_eq!(
            label_to_ontology_class(RegionLabel::Paragraph),
            "TextElement"
        );
        assert_eq!(label_to_ontology_class(RegionLabel::List), "ListElement");
        assert_eq!(label_to_ontology_class(RegionLabel::Table), "Table");
        assert_eq!(label_to_ontology_class(RegionLabel::Picture), "Picture");
        assert_eq!(label_to_ontology_class(RegionLabel::Caption), "Caption");
        assert_eq!(label_to_ontology_class(RegionLabel::Footnote), "Footnote");
        assert_eq!(
            label_to_ontology_class(RegionLabel::PageHeader),
            "PageHeader"
        );
        assert_eq!(
            label_to_ontology_class(RegionLabel::PageFooter),
            "PageFooter"
        );
        assert_eq!(label_to_ontology_class(RegionLabel::Formula), "Formula");
        assert_eq!(label_to_ontology_class(RegionLabel::Code), "CodeBlock");
    }
}
