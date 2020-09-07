extern crate tui;

mod items;

use std::io::{self, Write};

use crossterm::event::{read, Event, KeyCode, KeyEvent, KeyModifiers};

// use crate::items::Crate;
use crates_io_api::{Crate, CratesResponse, ListOptions, Sort, SyncClient};
use tui::backend::CrosstermBackend;
use tui::layout::{Alignment, Constraint, Direction, Layout};
use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Spans};
use tui::widgets::{
    self, Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Tabs, Widget,
};
use tui::Terminal;

pub const HELP: &str = r#"
                  __
.----.----.---.-.|  |_.-----.
|  __|   _|  _  ||   _|  -__|
|____|__| |___._||____|_____|
.-----.---.-.--------.-----.
|     |  _  |        |  -__|
|__|__|___._|__|__|__|_____|
.-----.-----.---.-.----.----.|  |--.
|__ --|  -__|  _  |   _|  __||     |
|_____|_____|___._|__| |____||__|__|

<C-h> toggle this help window

# search mode
<C-s> clear input
<Enter> perform the search and focus the results block
<Escape>, <C-r> focus the results block
<C-q> quit

# results mode
<C-s> focus the search bar
<j>, <k>, <up>, <down> move up and down the results
<Enter>, <C-c> go to crate (browser)
<C-g> go to repository (browser)
<C-q>, <q> quit



"#;

enum Mode {
    Search,
    Results,
}

#[derive(Default)]
struct Crates {
    items: Vec<Crate>,
    state: ListState,
}
impl Crates {
    fn new(items: Vec<Crate>) -> Self {
        Crates {
            items,
            state: ListState::default(),
        }
    }
    fn add(&mut self, item: Crate) {
        self.items.push(item);
    }
    fn select(&mut self, idx: Option<usize>) {
        if let Some(i) = idx {
            if i < self.items.len() {
                self.state.select(idx);
            } else {
                self.state.select(Some(self.items.len() - 1));
            }
        } else {
            self.state.select(idx);
        }
    }
    fn select_next(&mut self, n: Option<usize>) {
        self.select(self.state.selected().map(|i| i + n.unwrap_or(1)));
    }
    fn select_previous(&mut self, n: Option<usize>) {
        self.select(self.state.selected().map(|i| match i {
            0 => 0,
            _ => i - n.unwrap_or(1),
        }))
    }
}

fn main() -> Result<(), io::Error> {
    let stdout = io::stdout();
    crossterm::terminal::enable_raw_mode().unwrap();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut stdout = io::stdout();
    terminal.clear().unwrap();

    // create new client
    let client = SyncClient::new(
        "crate_name_search (github.com/adamsky/cns)",
        std::time::Duration::from_millis(1000),
    )
    .unwrap();

    let mut crates = Crates::default();

    let mut current_mode = Mode::Search;
    let mut show_intro = true;

    let mut search_block_title = "Search".to_string();
    let mut search_block_text = "this".to_string();
    let mut results_block_label = "Results".to_string();
    let mut results_current_tab = 0;

    let mut search_block_border_style = Style::default().fg(tui::style::Color::DarkGray);
    let mut results_block_border_style = Style::default().fg(tui::style::Color::DarkGray);
    let mut results_block_highlight_style = Style::default().bg(tui::style::Color::DarkGray);

    // support basic vim shortcuts like `gg` along with num prefixes
    let mut num_input = None;
    let mut last_key = KeyEvent {
        code: KeyCode::Null,
        modifiers: KeyModifiers::NONE,
    };
    let mut previous_key = KeyEvent {
        code: KeyCode::Null,
        modifiers: KeyModifiers::NONE,
    };

    loop {
        // handle mode-specific changes
        match current_mode {
            Mode::Search => {
                results_block_border_style = Style::default().fg(tui::style::Color::DarkGray);
                search_block_border_style = Style::default().fg(tui::style::Color::White);
            }
            Mode::Results => {
                results_block_border_style = Style::default().fg(tui::style::Color::White);
                search_block_border_style = Style::default().fg(tui::style::Color::DarkGray);
            }
        }

        // draw the interface
        terminal
            .draw(|f| {
                let chunks_horiz = Layout::default()
                    .direction(Direction::Horizontal)
                    .margin(3)
                    .constraints(
                        [
                            Constraint::Percentage(42),
                            Constraint::Max(4),
                            Constraint::Percentage(50),
                        ]
                        .as_ref(),
                    )
                    .split(f.size());
                let chunks_left = Layout::default()
                    .direction(Direction::Vertical)
                    // .margin(1)
                    .constraints([Constraint::Length(3), Constraint::Min(10)].as_ref())
                    .split(chunks_horiz[0]);
                let chunks_right = Layout::default()
                    .direction(Direction::Vertical)
                    // .margin(1)
                    .constraints([Constraint::Length(3), Constraint::Min(10)].as_ref())
                    .split(chunks_horiz[2]);
                let search_block = Block::default()
                    .title(search_block_title.as_str())
                    .borders(Borders::ALL)
                    .border_style(search_block_border_style);
                let paragraph = Paragraph::new(search_block_text.as_str()).block(search_block);
                f.render_widget(paragraph, chunks_left[0]);
                let results_block = Block::default()
                    .title(results_block_label.as_str())
                    .borders(Borders::ALL)
                    .border_style(results_block_border_style);

                let list_items: Vec<ListItem> = crates
                    .items
                    .iter()
                    .map(|i| ListItem::new(i.name.as_str()))
                    .collect();
                let results = List::new(list_items)
                    .block(results_block)
                    .highlight_style(results_block_highlight_style);
                f.render_stateful_widget(results, chunks_left[1], &mut crates.state);
                // f.render_widget(results, chunks_left[1]);

                if show_intro {
                    let intro = widgets::Paragraph::new(HELP)
                        .block(Block::default().borders(Borders::NONE));
                    f.render_widget(intro, chunks_horiz[2]);
                } else {
                    let top_bar = Block::default().title("").borders(Borders::BOTTOM);
                    let spans = Spans::from(vec![
                        Span::styled("My", Style::default().fg(Color::Yellow)),
                        Span::raw(" text"),
                    ]);
                    let titles = ["Summary", "Readme", "Repository", "Stats"]
                        .iter()
                        .cloned()
                        .map(Spans::from)
                        .collect();
                    let top_tabs = Tabs::new(titles)
                        .select(results_current_tab)
                        .block(Block::default().title("").borders(Borders::BOTTOM))
                        .style(Style::default().fg(Color::DarkGray))
                        .highlight_style(Style::default().fg(Color::White));
                    // .divider(tui::symbols::);
                    f.render_widget(top_tabs, chunks_right[0]);
                    let text = match crates.state.selected() {
                        Some(n) => format!(
                            "\n\
{},

{:?}
",
                            crates.items.get(n).unwrap().name,
                            crates.items.get(n).unwrap().description
                        ),
                        None => "nothing selected".to_string(),
                    };
                    // let name = Paragraph::new("\nthis = \"0.1.0\"").block(
                    //     Block::default().title("Cargo.toml").style(
                    //         Style::default()
                    //             .bg(tui::style::Color::Rgb(60, 60, 60))
                    //             .add_modifier(Modifier::BOLD),
                    //     ),
                    // );
                    // f.render_widget(name, chunks_right[0]);
                    let details = widgets::Paragraph::new(text.as_str())
                        // .style(Style::default().add_modifier())
                        .block(Block::default().borders(Borders::NONE));
                    f.render_widget(details, chunks_right[1]);
                }
            })
            .unwrap();

        if let Event::Key(key_event) = read().unwrap() {
            previous_key = last_key;
            last_key = key_event.clone();
            if let KeyEvent {
                code: kc,
                modifiers: mods,
            } = key_event
            {
                match current_mode {
                    Mode::Search => {
                        if mods == KeyModifiers::CONTROL {
                            match kc {
                                KeyCode::Char('h') => show_intro = !show_intro,
                                KeyCode::Char('r') => current_mode = Mode::Results,
                                KeyCode::Char('s') => {
                                    if search_block_text.len() > 0 {
                                        if let Some(space_idx) = search_block_text.find(" ") {
                                            search_block_text =
                                                search_block_text[0..space_idx].to_string();
                                        } else {
                                            search_block_text = "".to_string();
                                        }
                                    };
                                }
                                KeyCode::Char('q') => break,
                                //KeyCode::Char(c) => stdout.write_all(format!("{}", c).as_bytes()).unwrap(),
                                //Key::Alt(c) => println!("^{}", c),
                                //Key::Ctrl(c) => println!("*{}", c),
                                _ => (),
                            }
                        } else {
                            match kc {
                                KeyCode::Esc => current_mode = Mode::Results,
                                KeyCode::Char(k) => {
                                    search_block_text = format!("{}{}", search_block_text, k);
                                }
                                KeyCode::Backspace => {
                                    if search_block_text.len() > 0 {
                                        search_block_text = search_block_text
                                            [0..search_block_text.len() - 1]
                                            .to_string()
                                    };
                                }
                                KeyCode::Enter => {
                                    crates = Crates::new(Vec::new());
                                    crate_query(&search_block_text, &client)
                                        .unwrap()
                                        .into_iter()
                                        .for_each(|c| crates.add(c));
                                    crates.select(Some(0));
                                    show_intro = false;
                                    current_mode = Mode::Results;
                                }
                                //KeyCode::Char(c) => stdout.write_all(format!("{}", c).as_bytes()).unwrap(),
                                //Key::Alt(c) => println!("^{}", c),
                                //Key::Ctrl(c) => println!("*{}", c),
                                _ => (),
                            }
                        }
                    }
                    Mode::Results => {
                        if mods == KeyModifiers::CONTROL {
                            match kc {
                                KeyCode::Char('h') => show_intro = !show_intro,
                                KeyCode::Char('q') => break,
                                KeyCode::Char('s') => current_mode = Mode::Search,

                                _ => continue,
                            }
                        } else {
                            match kc {
                                KeyCode::Left => {
                                    if results_current_tab > 0 {
                                        results_current_tab -= 1
                                    }
                                }
                                KeyCode::Right => {
                                    if results_current_tab < 3 {
                                        results_current_tab += 1
                                    }
                                }
                                KeyCode::Up | KeyCode::Char('k') => match num_input {
                                    None => crates.select_previous(None),
                                    Some(n) => crates.select_previous(Some(n as usize)),
                                },
                                KeyCode::Down | KeyCode::Char('j') => match num_input {
                                    None => crates.select_next(None),
                                    Some(n) => crates.select_next(Some(n as usize)),
                                },
                                KeyCode::Char(ch) => match ch {
                                    '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' => {
                                        match num_input {
                                            None => num_input = ch.to_digit(10),
                                            Some(n) => {
                                                num_input = Some(n * 10 + ch.to_digit(10).unwrap())
                                            }
                                        }
                                        continue;
                                    }
                                    'g' => {
                                        if let KeyCode::Char('g') = last_key.code {
                                            if let KeyCode::Char('g') = previous_key.code {
                                                crates.select(Some(0));
                                            }
                                        }
                                    }
                                    'G' => crates.select(Some(crates.items.len() - 1)),
                                    'q' => break,
                                    _ => (),
                                },

                                _ => (),
                            }
                        }
                    }
                }
            }
        }
        // reset combos
        num_input = None;

        stdout.flush().unwrap();
    }

    terminal.clear().unwrap();
    Ok(())
}

fn crate_query(input: &str, client: &SyncClient) -> Result<Vec<Crate>, io::Error> {
    let opt = ListOptions {
        sort: Sort::Relevance,
        per_page: 20,
        page: 1,
        query: Some(input.to_string()),
    };
    let crates_response: CratesResponse = client.crates(opt).unwrap();
    Ok(crates_response.crates)

    // crates_result.crates.iter().next().unwrap().name

    // for c in summary.most_downloaded {
    //     println!("{}:", c.id);
    //     for dep in client.crate_dependencies(&c.id, &c.max_version)? {
    //         // Ignore optional dependencies.
    //         if !dep.optional {
    //             println!("    * {} - {}", dep.id, dep.version_id);
    //         }
    //     }
    // }
    // Ok(())
}

// fn key_is_num(key_code: KeyCode) -> bool {}
