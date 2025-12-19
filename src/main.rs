mod storage;

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, ListItem, Paragraph, Scrollbar, ScrollbarOrientation},
    DefaultTerminal, Frame,
};
use storage::{load_memos, Memo};
use tui_textarea::{Input, Key, TextArea};

enum CurrentView {
    List,
    Edit,
}

use tui_tree_widget::{Tree, TreeItem, TreeState};

struct App<'a> {
    view: CurrentView,
    memos: Vec<Memo>,
    tree_state: TreeState<String>,
    textarea: TextArea<'a>,
    should_quit: bool,
    // When editing, are we editing a new one or existing?
    // If existing, we update it. If new, we create it.
    // Actually we can just keep the 'current' memo being edited in the textarea
    // and when saving, we construct a Memo.
    // But wait, Memo::new creates a NEW path based on time.
    // If we edit an existing memo, we should overwrite the SAME path.
    // So we need to store the path of the memo being edited if it exists.
    editing_memo_path: Option<std::path::PathBuf>,
}

use regex::Regex;
use once_cell::sync::Lazy;

// Regex for Unordered list: ^(\s*)([-*+])\s+$ (check empty list item) or ^(\s*)([-*+])\s+(.*)
// Regex for Ordered list: ^(\s*)(\d+)\.\s+$ (check empty) or ^(\s*)(\d+)\.\s+(.*)

// We use slightly looser regex to capture prefix.
static UNORDERED_LIST_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(\s*)([-*+])\s+").unwrap());
static ORDERED_LIST_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(\s*)(\d+)\.\s+").unwrap());

impl App<'_> {
    fn new() -> Result<Self> {
        let memos = load_memos()?;
        // Initialize tree state with empty selection
        let mut tree_state = TreeState::default();
        if !memos.is_empty() {
             tree_state.select_first();
        }

        let mut textarea = TextArea::default();
        // Configure "Syntax Highlighting" (Hack using search pattern)
        // Matches:
        // 1. Headers: ^#{1,6} .*
        // 2. Bold: \*\*.*?\*\*
        // Note: Regex crate syntax.
        // We join patterns with |
        if let Err(e) = textarea.set_search_pattern("(^#{1,6} .+$)|(\\*\\*.+?\\*\\*)") {
            // Should not happen with valid regex, but log/ignore if it does
            eprintln!("Invalid regex: {}", e);
        }
        
        // Style: Bold and Cyan for highlighted text (Cyan works better on dark/light than plain Blue)
        textarea.set_search_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

        Ok(Self {
            view: CurrentView::List,
            memos,
            tree_state,
            textarea,
            should_quit: false,
            editing_memo_path: None,
        })
    }

    fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.should_quit {
            terminal.draw(|frame| self.draw(frame))?;
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    self.handle_key(key)?;
                }
            }
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        match self.view {
            CurrentView::List => self.draw_list(frame),
            CurrentView::Edit => self.draw_edit(frame),
        }
    }

    fn draw_list(&mut self, frame: &mut Frame) {
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
        // Root -> Memo File -> Headers -> Paragraphs
        
        let mut tree_items: Vec<TreeItem<String>> = Vec::new();

        for memo in &self.memos {
            // We flatly list memos.
            // To make it distinct, maybe we want to include the date in the label?
            // User just asked to remove hierarchy layer.
            // Previously: List View showed "Title (filename)".
            // Let's stick to Title, maybe append date/time if needed?
            // "Title (YYYY-MM-DD HH:mm:ss)" might be nice, but for now just Title as before or Title (filename).
            // Let's use Title (filename) to differentiate.
            // Filename is HH-mm-ss.md. Parent is YYYY-MM-DD.
            
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
        
        // Ensure formatting for items
        // We can't customize rendering per depth easily in tui-tree-widget 0.23 without custom render logic or wrapping.
        // But the indentation is handled by the widget.

        frame.render_stateful_widget(tree, main_layout[0], &mut self.tree_state);

        // --- Right: Preview ---
        // Showing preview for the *selected item*
        // If a header/paragraph is selected, we could show the whole memo or just that part.
        // For context, showing the whole memo is probably better, maybe scrolling to the part?
        // But Paragraph widget doesn't easily support "scroll to line".
        // Let's just show the whole memo content for any selection within that memo.
        
        let preview_text: Vec<Line> = if let Some(selected_id) = self.tree_state.selected().last() {
            // Check if selected_id is a memo path or a sub-item (memo_path::line)
            // IDs are constructed as "path" or "path::line_index".
            let path_str = selected_id.split("::").next().unwrap_or("");
            
            if let Some(memo) = self.memos.iter().find(|m| m.id == path_str || m.path.to_string_lossy() == path_str) {
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
    }

    fn draw_edit(&mut self, frame: &mut Frame) {
        let layout = Layout::default()
            .constraints([Constraint::Min(0), Constraint::Length(3)])
            .split(frame.area());

        self.textarea.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title(if self.editing_memo_path.is_some() { "Edit Memo" } else { "New Memo" }),
        );
        frame.render_widget(&self.textarea, layout[0]);

        let help_text = "Esc: Save & Exit | Ctrl+s: Save | Ctrl+c: Cancel";
        let help = Paragraph::new(help_text)
            .block(Block::default().borders(Borders::ALL))
            .style(Style::default().fg(Color::Gray));
        frame.render_widget(help, layout[1]);
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        match self.view {
            CurrentView::List => match key.code {
                KeyCode::Char('q') => self.should_quit = true,
                KeyCode::Down | KeyCode::Char('j') => {
                    self.tree_state.key_down();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.tree_state.key_up();
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    self.tree_state.key_left();
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    self.tree_state.key_right();
                }
                KeyCode::Char(' ') => { // Space to toggle
                    self.tree_state.toggle_selected();
                }
                KeyCode::Char('n') => {
                    self.view = CurrentView::Edit;
                    // Reset content but KEEP configuration
                    // TextArea::default() would lose our search pattern.
                    // Instead, we just clear lines.
                    self.textarea = TextArea::default(); 
                    // Re-apply configuration (or clean way: create a helper `new_textarea()`)
                    // Let's just re-apply for now to be safe and simple.
                    if let Err(_) = self.textarea.set_search_pattern("(^#{1,6} .+$)|(\\*\\*.+?\\*\\*)") { }
                    self.textarea.set_search_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
                    
                    self.editing_memo_path = None;
                }
                KeyCode::Enter | KeyCode::Char('e') => {
                    if let Some(selected_id) = self.tree_state.selected().last() {
                         // Check if it matches a memo ID exactly or is a child of a memo
                         let path_str = selected_id.split("::").next().unwrap_or("");
                         
                         if let Some(memo) = self.memos.iter().find(|m| m.id == path_str) {
                            // If selected is exactly the memo, or a part of it, we open the memo.
                            // However, if we selected a specific header, maybe jump to that line?
                            // TextArea doesn't easily support "scroll to line" without cursor manipulation.
                            // Let's just open the memo for now.
                            
                            self.view = CurrentView::Edit;
                            let lines: Vec<String> = memo.content.lines().map(|s| s.to_string()).collect();
                            self.textarea = TextArea::new(lines);
                            
                            // If a specific line was selected (ID has ::line_idx), move cursor there
                            if let Some(line_part) = selected_id.split("::").nth(1) {
                                if let Ok(line_idx) = line_part.parse::<usize>() {
                                    // Move cursor to that line
                                    self.textarea.move_cursor(tui_textarea::CursorMove::Jump(line_idx as u16, 0));
                                }
                            }

                            if let Err(_) = self.textarea.set_search_pattern("(^#{1,6} .+$)|(\\*\\*.+?\\*\\*)") { }
                            self.textarea.set_search_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

                            self.editing_memo_path = Some(memo.path.clone());
                         } else {
                            // Directory or unknown
                            self.tree_state.toggle_selected();
                         }
                    }
                }
                KeyCode::Char('d') => {
                    if let Some(selected_id) = self.tree_state.selected().last() {
                         let path_str = selected_id.split("::").next().unwrap_or("");
                         if let Some(memo) = self.memos.iter().find(|m| m.id == path_str) {
                             memo.delete()?;
                             self.reload_memos()?;
                         }
                    }
                }
                _ => {}
            },
            CurrentView::Edit => {
                match key.code {
                    KeyCode::Esc => {
                        self.save_current_memo()?;
                        self.view = CurrentView::List;
                        self.reload_memos()?;
                    }
                    KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                         self.save_current_memo()?;
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.view = CurrentView::List;
                    }
                    KeyCode::Enter => {
                        // Handle auto-list logic
                        let cursor = self.textarea.cursor();
                        let current_row = cursor.0;
                        
                        // Clone the current line to avoid holding an immutable borrow of textarea
                        // while we want to mutate it later.
                        let current_line = if current_row < self.textarea.lines().len() {
                             self.textarea.lines()[current_row].clone()
                        } else {
                            String::new()
                        };

                        // Check Ordered List first
                        if let Some(caps) = ORDERED_LIST_RE.captures(&current_line) {
                            let old_indent = &caps[1].to_string(); // clone captures to own strings
                            let number_str = &caps[2];
                            let full_match = &caps[0];
                            let full_len = full_match.len();
                            let trimmed_len = current_line.trim_end().len();

                            // Check if line is "empty" (just the list marker)
                            if trimmed_len == full_len {
                                // Empty list item -> Remove the list marker (clear line)
                                self.textarea.delete_line_by_head();
                            } else {
                                // Append newline
                                self.textarea.insert_newline();
                                // Calculate next number
                                let num: usize = number_str.parse().unwrap_or(0);
                                let new_prefix = format!("{}{}. ", old_indent, num + 1);
                                self.textarea.insert_str(new_prefix);
                            }
                        } else if let Some(caps) = UNORDERED_LIST_RE.captures(&current_line) {
                            // let old_indent = &caps[1];
                            // let bullet = &caps[2];
                            let full_match = &caps[0];
                            let full_len = full_match.len();
                            let trimmed_len = current_line.trim_end().len();

                            if trimmed_len == full_len {
                                // Empty list item -> Remove
                                self.textarea.delete_line_by_head();
                            } else {
                                // Append newline
                                self.textarea.insert_newline();
                                // Re-insert same prefix
                                let prefix_str = full_match.to_string();
                                self.textarea.insert_str(prefix_str);
                            }
                        } else {
                            // Normal Enter
                            self.textarea.input(Input::from(key));
                        }
                    }
                    _ => {
                        let input: Input = key.into();
                        self.textarea.input(input);
                    }
                }
            }
        }
        Ok(())
    }

    fn save_current_memo(&mut self) -> Result<()> {
        let content = self.textarea.lines().join("\n");
        if content.trim().is_empty() {
            if self.editing_memo_path.is_none() {
                return Ok(());
            }
        }

        let memo = if let Some(path) = &self.editing_memo_path {
            Memo {
                path: path.clone(),
                content,
                id: path.to_string_lossy().to_string(),
            }
        } else {
            Memo::new(content)
        };
        memo.save()?; 
        self.editing_memo_path = Some(memo.path);
        
        Ok(())
    }

    fn reload_memos(&mut self) -> Result<()> {
        self.memos = load_memos()?;
        // Ideally we want to preserve selection, but tree structure might change.
        // For simplicity, just ensure something is selected if possible.
        // But re-selecting root is safe.
        // self.tree_state = TreeState::default(); // Reset state or keep? 
        // If we keep state, invalid IDs might remain. 
        // tui-tree-widget handles selection loosely (Vec<String>). 
        // Let's reset for now to be safe, or just check validity.
        // Or better: Just re-load memos. TreeState uses String IDs. 
        // If a file was deleted, its ID is gone.
        // Let's reset selection if empty.
        if self.memos.is_empty() {
             self.tree_state = TreeState::default(); 
        } else {
             // If nothing selected, select first
             if self.tree_state.selected().is_empty() {
                 self.tree_state.select_first(); 
             }
        }
        Ok(())
    }
}

// --- Helper for Markdown Parsing ---
fn parse_markdown_to_tree(content: &str, memo_id: &str) -> Vec<TreeItem<'static, String>> {
    fn parse_recursive(
        iter: &mut std::iter::Peekable<std::slice::Iter<(usize, &str)>>, 
        min_level: usize,
        memo_id: &str
    ) -> Vec<TreeItem<'static, String>> {
        let mut items = Vec::new();
        
        while let Some(&(i, line)) = iter.peek() {
            let trimmed = line.trim();
            if trimmed.is_empty() { 
                iter.next(); 
                continue; 
            }
            
            let (level, text) = if trimmed.starts_with("# ") { (1, trimmed[2..].to_string()) }
            else if trimmed.starts_with("## ") { (2, trimmed[3..].to_string()) }
            else if trimmed.starts_with("### ") { (3, trimmed[4..].to_string()) }
            else if trimmed.starts_with("#### ") { (4, trimmed[5..].to_string()) }
            else if trimmed.starts_with("##### ") { (5, trimmed[6..].to_string()) }
            else if trimmed.starts_with("###### ") { (6, trimmed[7..].to_string()) }
            else { (7, trimmed.to_string()) }; // 7 = Paragraph
            
            if level < min_level {
                // Return to parent
                break;
            }
            
            // Consume this line
            iter.next();
            
            let children = if level < 7 {
                parse_recursive(iter, level + 1, memo_id)
            } else {
                Vec::new()
            };
            
            let id = format!("{}::{}", memo_id, i);
            let item = if children.is_empty() {
                TreeItem::new_leaf(id, text)
            } else {
                TreeItem::new(id, text, children).unwrap()
            };
            items.push(item);
        }
        items
    }
    
    let lines: Vec<(usize, &str)> = content.lines().enumerate().collect();
    let mut iter = lines.iter().peekable();
    parse_recursive(&mut iter, 1, memo_id)
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let mut terminal = ratatui::init();
    let app = App::new()?;
    let result = app.run(&mut terminal);
    ratatui::restore();
    result
}
