//! # Incremental Rendering Performance Demo
//!
//! This demo compares full screen redraw vs. incremental rendering performance.
//! It simulates typing characters and shows:
//! - Characters written per frame (before/after)
//! - Lines redrawn per frame (before/after)

use text_renderer_host::TextRenderer;
use view_types::{CursorPosition, ViewContent, ViewFrame, ViewId, ViewKind};

fn create_text_frame(lines: Vec<String>, cursor: Option<CursorPosition>, revision: u64) -> ViewFrame {
    let mut frame = ViewFrame::new(
        ViewId::new(),
        ViewKind::TextBuffer,
        revision,
        ViewContent::text_buffer(lines),
        0,
    );
    if let Some(cursor_pos) = cursor {
        frame = frame.with_cursor(cursor_pos);
    }
    frame
}

fn main() {
    println!("=== Incremental Rendering Performance Demo ===\n");
    println!("Simulating typing 'test' character by character...\n");

    // Scenario: Start with empty buffer, type "test" one char at a time
    let scenarios = vec![
        (vec!["".to_string()], Some(CursorPosition::new(0, 0)), "Initial empty buffer"),
        (vec!["t".to_string()], Some(CursorPosition::new(0, 1)), "Typed 't'"),
        (vec!["te".to_string()], Some(CursorPosition::new(0, 2)), "Typed 'e'"),
        (vec!["tes".to_string()], Some(CursorPosition::new(0, 3)), "Typed 's'"),
        (vec!["test".to_string()], Some(CursorPosition::new(0, 4)), "Typed 't'"),
    ];

    println!("=== FULL REDRAW MODE (baseline) ===");
    let mut renderer_full = TextRenderer::new();
    let mut total_chars_full = 0;

    for (i, (lines, cursor, description)) in scenarios.iter().enumerate() {
        let frame = create_text_frame(lines.clone(), *cursor, i as u64 + 1);
        let _output = renderer_full.render_snapshot(Some(&frame), None);
        let stats = renderer_full.stats();
        
        println!(
            "Frame {}: {} - chars_written={}, lines_redrawn=N/A",
            i + 1,
            description,
            stats.chars_written_per_frame
        );
        total_chars_full += stats.chars_written_per_frame;
    }

    println!("\nTotal chars written (full redraw): {}", total_chars_full);

    println!("\n=== INCREMENTAL RENDER MODE (optimized) ===");
    let mut renderer_incr = TextRenderer::new();
    let mut total_chars_incr = 0;
    let mut total_lines_incr = 0;

    for (i, (lines, cursor, description)) in scenarios.iter().enumerate() {
        let frame = create_text_frame(lines.clone(), *cursor, i as u64 + 1);
        let _output = renderer_incr.render_incremental(Some(&frame), None);
        let stats = renderer_incr.stats();
        
        println!(
            "Frame {}: {} - chars_written={}, lines_redrawn={}",
            i + 1,
            description,
            stats.chars_written_per_frame,
            stats.lines_redrawn_per_frame
        );
        total_chars_incr += stats.chars_written_per_frame;
        total_lines_incr += stats.lines_redrawn_per_frame;
    }

    println!("\nTotal chars written (incremental): {}", total_chars_incr);
    println!("Total lines redrawn (incremental): {}", total_lines_incr);

    // Calculate improvement
    let improvement = if total_chars_full > 0 {
        ((total_chars_full - total_chars_incr) as f64 / total_chars_full as f64) * 100.0
    } else {
        0.0
    };

    println!("\n=== PERFORMANCE IMPROVEMENT ===");
    println!("Characters written reduction: {:.1}%", improvement);
    println!("Incremental rendering writes only changed content!");

    println!("\n=== CURSOR-ONLY MOVEMENT TEST ===");
    println!("Simulating cursor moving without content change...\n");

    let content = vec!["hello world".to_string()];
    let mut renderer_cursor = TextRenderer::new();

    // Initial render
    let frame1 = create_text_frame(content.clone(), Some(CursorPosition::new(0, 0)), 1);
    renderer_cursor.render_incremental(Some(&frame1), None);
    println!("Initial: chars_written={}, lines_redrawn={}", 
             renderer_cursor.stats().chars_written_per_frame,
             renderer_cursor.stats().lines_redrawn_per_frame);

    // Move cursor without changing content
    let frame2 = create_text_frame(content.clone(), Some(CursorPosition::new(0, 5)), 2);
    renderer_cursor.render_incremental(Some(&frame2), None);
    println!("Cursor moved: chars_written={}, lines_redrawn={}", 
             renderer_cursor.stats().chars_written_per_frame,
             renderer_cursor.stats().lines_redrawn_per_frame);

    println!("\n✓ Cursor-only moves should have minimal overhead!");
    
    println!("\n=== MULTI-LINE EDITING TEST ===");
    println!("Editing a multi-line document...\n");

    let mut renderer_multi = TextRenderer::new();
    
    // Initial 5-line document
    let lines1 = vec![
        "line 1".to_string(),
        "line 2".to_string(),
        "line 3".to_string(),
        "line 4".to_string(),
        "line 5".to_string(),
    ];
    let frame = create_text_frame(lines1, Some(CursorPosition::new(0, 0)), 1);
    renderer_multi.render_incremental(Some(&frame), None);
    println!("Initial render: chars_written={}, lines_redrawn={}", 
             renderer_multi.stats().chars_written_per_frame,
             renderer_multi.stats().lines_redrawn_per_frame);

    // Edit only line 2
    let lines2 = vec![
        "line 1".to_string(),
        "line 2 modified".to_string(),
        "line 3".to_string(),
        "line 4".to_string(),
        "line 5".to_string(),
    ];
    let frame = create_text_frame(lines2, Some(CursorPosition::new(1, 15)), 2);
    renderer_multi.render_incremental(Some(&frame), None);
    println!("Edit line 2: chars_written={}, lines_redrawn={}", 
             renderer_multi.stats().chars_written_per_frame,
             renderer_multi.stats().lines_redrawn_per_frame);

    println!("\n✓ Only modified line should be redrawn (1 line out of 5)!");

    println!("\n=== DEMO COMPLETE ===");
    println!("\nKey Takeaways:");
    println!("• Incremental rendering dramatically reduces characters written");
    println!("• Only changed lines are redrawn (O(changes) instead of O(viewport))");
    println!("• Cursor-only moves have minimal overhead");
    println!("• This makes typing feel instant even in large documents");
}
