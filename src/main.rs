extern crate tui;

mod items;
mod logo;

use std::io::{self, Write};

use crossterm::event::{read, Event, KeyCode, KeyEvent};

use tui::backend::CrosstermBackend;
use tui::layout::{Constraint, Direction, Layout};
use tui::widgets::{self, Block, Borders, List, ListItem, Widget};
use tui::Terminal;

fn main() -> Result<(), io::Error> {
    let stdout = io::stdout();
    crossterm::terminal::enable_raw_mode().unwrap();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut stdout = io::stdout();
    terminal.clear().unwrap();

    let mut crate_list = Vec::new();
    crate_list.push(items::Crate {
        name: "test".to_string(),
        description: "test_desc".to_string(),
        version_latest: "0.1.0".to_string(),
    });
    crate_list.push(items::Crate {
        name: "test".to_string(),
        description: "test_desc".to_string(),
        version_latest: "0.1.0".to_string(),
    });

    loop {
        terminal
            .draw(|f| {
                let chunks_horiz = Layout::default()
                    .direction(Direction::Horizontal)
                    .margin(1)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
                    .split(f.size());
                let chunks_left = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(1)
                    .constraints([Constraint::Length(3), Constraint::Min(10)].as_ref())
                    .split(chunks_horiz[0]);
                //let chunks_right = Layout::default();
                let search_block = Block::default().title("Search").borders(Borders::ALL);
                f.render_widget(search_block, chunks_left[0]);
                let results_block = Block::default().title("Results").borders(Borders::ALL);

                let list_items: Vec<ListItem> = crate_list
                    .iter()
                    .map(|i| ListItem::new(i.name.as_str()))
                    .collect();
                let results = List::new(list_items).block(results_block);
                f.render_widget(results, chunks_left[1]);

                let logo = widgets::Paragraph::new(logo::CNS)
                    .block(Block::default().borders(Borders::NONE));
                f.render_widget(logo, chunks_horiz[1]);
            })
            .unwrap();

        if let Event::Key(KeyEvent { code: kc, .. }) = read().unwrap() {
            match kc {
                KeyCode::Char('q') => return Ok(()),
                //KeyCode::Char(c) => stdout.write_all(format!("{}", c).as_bytes()).unwrap(),
                //Key::Alt(c) => println!("^{}", c),
                //Key::Ctrl(c) => println!("*{}", c),
                _ => continue,
            }
        }
        stdout.flush().unwrap();
    }
    //Ok(())
}
