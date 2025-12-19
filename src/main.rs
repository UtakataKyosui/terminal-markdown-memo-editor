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
        // Root -> Date Dir -> Memo File (Title)
        
        // Group memos by parent directory (Date)
        // We assume memos are strictly YYYY-MM-DD/HH-mm-ss.md
        let mut tree_items: Vec<TreeItem<String>> = Vec::new();
        let mut current_date: Option<String> = None;
        let mut current_date_items: Vec<TreeItem<String>> = Vec::new();

        // Memos are sorted by path descending (newest first).
        for memo in &self.memos {
            // parent dir name
             if let Some(parent) = memo.path.parent() {
                let date_name = parent.file_name().unwrap_or_default().to_string_lossy().to_string();
                
                if Some(&date_name) != current_date.as_ref() {
                    // Push previous date group
                    if let Some(d) = current_date {
                        tree_items.push(TreeItem::new(d.clone(), d, current_date_items).unwrap());
                    }
                    current_date = Some(date_name);
                    current_date_items = Vec::new();
                }

                let title = memo.title();
                let leaf_item = TreeItem::new_leaf(memo.id.clone(), title); // Use full path/ID as leaf ID
                current_date_items.push(leaf_item);
             }
        }
        // Push last group
        if let Some(d) = current_date {
            tree_items.push(TreeItem::new(d.clone(), d, current_date_items).unwrap());
        }

        let tree = Tree::new(&tree_items).unwrap()
            .block(Block::default().borders(Borders::ALL).title("Memos"))
            .highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
            .experimental_scrollbar(Some(ratatui::widgets::Scrollbar::new(ratatui::widgets::ScrollbarOrientation::VerticalRight)));

        frame.render_stateful_widget(tree, main_layout[0], &mut self.tree_state);

        // --- Right: Preview ---
        let block = Block::default().borders(Borders::ALL).title("Preview");
        
        let preview_content: Vec<Line> = if let Some(selected_ids) = self.tree_state.selected().last() {
             // Find memo by ID (which is path string)
             if let Some(memo) = self.memos.iter().find(|m| m.id == *selected_ids) {
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
                 vec![Line::from("Directory selected")]
             }
        } else {
            vec![Line::from("No memo selected")]
        };

        // Use Paragraph for scrollable text? For now just simple Paragraph.
        // We might want to support scrolling in preview later, but for now just show top.
        let preview = Paragraph::new(preview_content)
            .block(block)
            .wrap(ratatui::widgets::Wrap { trim: false });
        
        frame.render_widget(preview, main_layout[1]);

        // --- Bottom: Help ---
        let help_text = "n: New | Enter: Edit/Toggle | d: Delete | q: Quit | h/l/j/k or Arrows: Navigate";
        let help = Paragraph::new(help_text)
            .block(Block::default().borders(Borders::ALL))
            .style(Style::default().fg(Color::Gray));
        frame.render_widget(help, outer_layout[1]);
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
                         // Find memo by ID
                         if let Some(memo) = self.memos.iter().find(|m| m.id == *selected_id) {
                            self.view = CurrentView::Edit;
                            let lines: Vec<String> = memo.content.lines().map(|s| s.to_string()).collect();
                            self.textarea = TextArea::new(lines);
                            // Re-apply configuration
                            if let Err(_) = self.textarea.set_search_pattern("(^#{1,6} .+$)|(\\*\\*.+?\\*\\*)") { }
                            self.textarea.set_search_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

                            self.editing_memo_path = Some(memo.path.clone());
                         } else {
                            // Directory selected? Toggle it
                            self.tree_state.toggle_selected();
                         }
                    }
                }
                KeyCode::Char('d') => {
                    if let Some(selected_id) = self.tree_state.selected().last() {
                         // Find memo
                         if let Some(memo) = self.memos.iter().find(|m| m.id == *selected_id) {
                             memo.delete()?;
                             self.reload_memos()?;
                             // Tree state might be invalid if we deleted last item in folder, but reload resets?
                             // Ideally preserve selection or move to next. For now, reload resets slightly.
                             // Actually reload_memos just reloads memos, tree state is ID based.
                             // We might need to handle stale ID in tree state?
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

fn main() -> Result<()> {
    color_eyre::install()?;
    let mut terminal = ratatui::init();
    let app = App::new()?;
    let result = app.run(&mut terminal);
    ratatui::restore();
    result
}
