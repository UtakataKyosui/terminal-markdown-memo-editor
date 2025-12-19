use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use ratatui::DefaultTerminal;
use std::path::PathBuf;
use tui_textarea::{Input, TextArea};
use tui_tree_widget::TreeState;
use regex::Regex;
use once_cell::sync::Lazy;

use crate::storage::{Memo, load_memos};
use crate::ui; // We will create this

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum CurrentView {
    List,
    Edit,
}

// Regex for Unordered list: ^(\s*)([-*+])\s+$ (check empty list item) or ^(\s*)([-*+])\s+(.*)
// Regex for Ordered list: ^(\s*)(\d+)\.\s+$ (check empty) or ^(\s*)(\d+)\.\s+(.*)
static UNORDERED_LIST_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(\s*)([-*+])\s+").unwrap());
static ORDERED_LIST_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(\s*)(\d+)\.\s+").unwrap());

const SEARCH_PATTERN: &str = "(^#{1,6} .+$)|(\\*\\*.+?\\*\\*)";

pub struct App<'a> {
    pub should_quit: bool,
    pub view: CurrentView,
    pub memos: Vec<Memo>,
    pub tree_state: TreeState<String>,
    pub textarea: TextArea<'a>,
    pub editing_memo_path: Option<PathBuf>,
}

impl<'a> App<'a> {
    pub fn new() -> Result<Self> {
        let memos = load_memos()?;
        // Initialize tree state with empty selection
        let mut tree_state = TreeState::default();
        let root_ids: Vec<String> = memos.iter().map(|m| m.id.clone()).collect();
        tree_state.open(root_ids);
        
        if !memos.is_empty() {
             tree_state.select_first();
        }

        let mut textarea = TextArea::default();
        Self::configure_textarea(&mut textarea);

        Ok(Self {
            should_quit: false,
            view: CurrentView::List,
            memos,
            tree_state,
            textarea,
            editing_memo_path: None,
        })
    }

    fn configure_textarea(textarea: &mut TextArea<'a>) {
        let _ = textarea.set_search_pattern(SEARCH_PATTERN);
        textarea.set_search_style(ratatui::style::Style::default().fg(ratatui::style::Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD));
    }

    pub fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.should_quit {
            terminal.draw(|frame| ui::draw(&mut self, frame))?;
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    self.handle_key(key)?;
                }
            }
        }
        Ok(())
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
                    self.textarea = TextArea::default(); 
                    Self::configure_textarea(&mut self.textarea);
                    
                    self.editing_memo_path = None;
                }
                KeyCode::Enter | KeyCode::Char('e') => {
                    if let Some(selected_id) = self.tree_state.selected().last() {
                         // Check if it matches a memo ID exactly or is a child of a memo
                         let path_str = selected_id.split("::").next().unwrap_or("");
                         
                         if let Some(memo) = self.memos.iter().find(|m| m.id == path_str) {
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

                            Self::configure_textarea(&mut self.textarea);

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
                        
                        let current_line = if current_row < self.textarea.lines().len() {
                             self.textarea.lines()[current_row].clone()
                        } else {
                            String::new()
                        };

                        if let Some(caps) = ORDERED_LIST_RE.captures(&current_line) {
                            let old_indent = &caps[1].to_string();
                            let number_str = &caps[2];
                            let full_match = &caps[0];
                            let full_len = full_match.len();
                            let trimmed_len = current_line.trim_end().len();

                            if trimmed_len == full_len {
                                self.textarea.delete_line_by_head();
                            } else {
                                self.textarea.insert_newline();
                                let num: usize = number_str.parse().unwrap_or(0);
                                let new_prefix = format!("{}{}. ", old_indent, num + 1);
                                self.textarea.insert_str(new_prefix);
                            }
                        } else if let Some(caps) = UNORDERED_LIST_RE.captures(&current_line) {
                            let full_match = &caps[0];
                            let full_len = full_match.len();
                            let trimmed_len = current_line.trim_end().len();

                            if trimmed_len == full_len {
                                self.textarea.delete_line_by_head();
                            } else {
                                self.textarea.insert_newline();
                                let prefix_str = full_match.to_string();
                                self.textarea.insert_str(prefix_str);
                            }
                        } else {
                            self.textarea.input(Input::from(key));
                        }
                    }
                    _ => {
                        self.textarea.input(Input::from(key));
                    }
                }
            }
        }
        Ok(())
    }

    fn save_current_memo(&mut self) -> Result<()> {
        let content = self.textarea.lines().join("\n");
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
        
        let mut new_state = TreeState::default();
        let root_ids: Vec<String> = self.memos.iter().map(|m| m.id.clone()).collect();
        new_state.open(root_ids);

        if !self.memos.is_empty() {
             new_state.select_first();
        }
        
        self.tree_state = new_state;
        Ok(())
    }
}
