mod storage;
mod markdown;
mod app;
mod ui;

use color_eyre::Result;
use app::App;

fn main() -> Result<()> {
    color_eyre::install()?;
    let mut terminal = ratatui::init();
    let app = App::new()?;
    let result = app.run(&mut terminal);
    ratatui::restore();
    result
}
