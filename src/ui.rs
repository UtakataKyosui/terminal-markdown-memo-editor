use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use tui_tree_widget::{Tree, TreeItem};

use crate::app::{App, CurrentView};
use crate::markdown::parse_markdown_to_tree;

pub fn draw(app: &mut App, frame: &mut Frame) {
    match app.view {
        CurrentView::List => draw_list(app, frame),
        CurrentView::Edit => draw_edit(app, frame),
    }
}

fn draw_list(app: &mut App, frame: &mut Frame) {
    // Vertical layout: [Main Area (Tree + Preview)] | [Help]
    let outer_layout = Layout::default()
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(frame.area());

    // Main Area: [Tree (30%)] | [Preview (70%)]
    let main_layout = Layout::default()
        .direction(ratatui::layout::Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(outer_layout[0]);

    // --- Left: Memo Tree ---
    // Construct Tree Items:
    // Root -> Memo File -> Headers
    
    let mut tree_items: Vec<TreeItem<String>> = Vec::new();

    for memo in &app.memos {
        // We flatly list memos.
        let date_str = memo.path.parent().and_then(|p| p.file_name()).map(|s| s.to_string_lossy()).unwrap_or_default();
        let time_str = memo.path.file_stem().map(|s| s.to_string_lossy()).unwrap_or_default();
        let display_date = format!("{} {}", date_str, time_str);

        let title = memo.title();
        let label = format!("{} ({})", title, display_date);

        // Parse memo content into tree items
        let memo_children = parse_markdown_to_tree(&memo.content, &memo.id);
        
        // Memo file is now a node with children (if any), otherwise leaf
        let memo_item = if memo_children.is_empty() {
            TreeItem::new_leaf(memo.id.clone(), label)
        } else {
            TreeItem::new(memo.id.clone(), label, memo_children).expect("memo id duplicate?")
        };
        
        tree_items.push(memo_item);
    }

    let tree = Tree::new(&tree_items).unwrap()
        .block(Block::default().borders(Borders::ALL).title("Memos"))
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
        .experimental_scrollbar(Some(ratatui::widgets::Scrollbar::new(ratatui::widgets::ScrollbarOrientation::VerticalRight)));
    
    frame.render_stateful_widget(tree, main_layout[0], &mut app.tree_state);

    // --- Right: Preview ---
    // Showing preview for the *selected item*
    let preview_text: Vec<Line> = if let Some(selected_id) = app.tree_state.selected().last() {
        // Check if selected_id is a memo path or a sub-item (memo_path::line)
        let path_str = selected_id.split("::").next().unwrap_or("");
        
        if let Some(memo) = app.memos.iter().find(|m| m.id == path_str || m.path.to_string_lossy() == path_str) {
             // Found the memo
             memo.content.lines().map(|line| {
                if line.starts_with("# ") {
                    Line::from(Span::styled(
                        line, 
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                    ))
                } else if line.starts_with("## ") {
                     Line::from(Span::styled(
                        line, 
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    ))
                } else if line.starts_with("### ") {
                     Line::from(Span::styled(
                        line, 
                        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                    ))
                } else if line.starts_with("- ") || line.starts_with("* ") {
                     Line::from(Span::styled(
                        line, 
                        Style::default().fg(Color::White)
                    ))
                } else {
                    Line::from(line)
                }
            }).collect()
        } else {
             if path_str.contains(std::path::MAIN_SEPARATOR) {
                  vec![Line::from(format!("Selected: {}", selected_id))]
             } else {
                  vec![Line::from("Directory selected")]
             }
        }
    } else {
        vec![Line::from("No selection")]
    };

    let preview = Paragraph::new(preview_text)
        .block(Block::default().borders(Borders::ALL).title("Preview"))
        .wrap(ratatui::widgets::Wrap { trim: false });
    
    frame.render_widget(preview, main_layout[1]);

    // --- Bottom: Help ---
    let help_text = "n: 新規作成 | Enter: 編集/展開 | Space/→: 展開/折畳 | d: 削除 | q: 終了 | ↑↓: 移動";
    let help = Paragraph::new(help_text)
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(Color::Gray));
    frame.render_widget(help, outer_layout[1]);
}

fn draw_edit(app: &mut App, frame: &mut Frame) {
    let layout = Layout::default()
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(frame.area());

    app.textarea.set_block(
        Block::default()
            .borders(Borders::ALL)
            .title(if app.editing_memo_path.is_some() { "Edit Memo" } else { "New Memo" }),
    );
    frame.render_widget(&app.textarea, layout[0]);

    let help_text = "Esc: Save & Exit | Ctrl+s: Save | Ctrl+c: Cancel";
    let help = Paragraph::new(help_text)
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(Color::Gray));
    frame.render_widget(help, layout[1]);
}
