use tui_tree_widget::TreeItem;

// --- Helper for Markdown Parsing ---
pub fn parse_markdown_to_tree(content: &str, memo_id: &str) -> Vec<TreeItem<'static, String>> {
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
            
            let (level, text) = if let Some(stripped) = trimmed.strip_prefix("# ") { (1, stripped.to_string()) }
            else if let Some(stripped) = trimmed.strip_prefix("## ") { (2, stripped.to_string()) }
            else if let Some(stripped) = trimmed.strip_prefix("### ") { (3, stripped.to_string()) }
            else if let Some(stripped) = trimmed.strip_prefix("#### ") { (4, stripped.to_string()) }
            else if let Some(stripped) = trimmed.strip_prefix("##### ") { (5, stripped.to_string()) }
            else if let Some(stripped) = trimmed.strip_prefix("###### ") { (6, stripped.to_string()) }
            else { (7, trimmed.to_string()) }; // 7 = Paragraph
            
            if level < min_level {
                // Return to parent
                break;
            }
            
            // Consume this line
            iter.next();
            
            // Ignore paragraphs (level 7) in tree view
            if level == 7 {
                continue;
            }
            
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
    
    // Skip the first non-empty line (Title) as it is already displayed as the Memo Root
    while let Some(&(_, line)) = iter.peek() {
        if !line.trim().is_empty() {
             iter.next();
             break;
        }
        iter.next();
    }

    parse_recursive(&mut iter, 1, memo_id)
}
