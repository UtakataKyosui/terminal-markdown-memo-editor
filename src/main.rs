mod storage;

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    DefaultTerminal, Frame,
};
use storage::{load_memos, Memo};
use tui_textarea::{Input, Key, TextArea};

enum CurrentView {
    List,
    Edit,
}

struct App<'a> {
    view: CurrentView,
    memos: Vec<Memo>,
    list_state: ListState,
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
        let mut list_state = ListState::default();
        if !memos.is_empty() {
            list_state.select(Some(0));
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
            list_state,
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
        let layout = Layout::default()
            .constraints([Constraint::Min(0), Constraint::Length(3)])
            .split(frame.area());

        let items: Vec<ListItem> = self
            .memos
            .iter()
            .map(|memo| {
                let title = memo.title();
                ListItem::new(Line::from(vec![
                    Span::styled(title, Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(format!(" ({})", memo.path.file_name().unwrap_or_default().to_string_lossy())),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Memos"))
            .highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
            .highlight_symbol(">> ");

        frame.render_stateful_widget(list, layout[0], &mut self.list_state);

        let help_text = "n: New | e/Enter: Edit | d: Delete | q: Quit | ↑/↓: Navigate";
        let help = Paragraph::new(help_text)
            .block(Block::default().borders(Borders::ALL))
            .style(Style::default().fg(Color::Gray));
        frame.render_widget(help, layout[1]);
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
                    if !self.memos.is_empty() {
                        let i = match self.list_state.selected() {
                            Some(i) => {
                                if i >= self.memos.len() - 1 {
                                    0
                                } else {
                                    i + 1
                                }
                            }
                            None => 0,
                        };
                        self.list_state.select(Some(i));
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if !self.memos.is_empty() {
                        let i = match self.list_state.selected() {
                            Some(i) => {
                                if i == 0 {
                                    self.memos.len() - 1
                                } else {
                                    i - 1
                                }
                            }
                            None => 0,
                        };
                        self.list_state.select(Some(i));
                    }
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
                    if let Some(i) = self.list_state.selected() {
                        if let Some(memo) = self.memos.get(i) {
                            self.view = CurrentView::Edit;
                            let lines: Vec<String> = memo.content.lines().map(|s| s.to_string()).collect();
                            self.textarea = TextArea::new(lines);
                            // Re-apply configuration
                            if let Err(_) = self.textarea.set_search_pattern("(^#{1,6} .+$)|(\\*\\*.+?\\*\\*)") { }
                            self.textarea.set_search_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

                            self.editing_memo_path = Some(memo.path.clone());
                        }
                    }
                }
                KeyCode::Char('d') => {
                    if let Some(i) = self.list_state.selected() {
                        if let Some(memo) = self.memos.get(i) {
                            memo.delete()?;
                            self.reload_memos()?;
                            if self.memos.is_empty() {
                                self.list_state.select(None);
                            } else if i >= self.memos.len() {
                                self.list_state.select(Some(self.memos.len() - 1));
                            }
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
        if self.list_state.selected().is_none() && !self.memos.is_empty() {
            self.list_state.select(Some(0));
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
