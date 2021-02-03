#![allow(unused)]

extern crate curl;
extern crate tui;
extern crate webbrowser;

mod items;

use std::io::{self, Write};

use crates_io_api::{CrateResponse, CratesResponse, ListOptions, Sort, SyncClient};
use crossterm::event::{read, Event, KeyCode, KeyEvent, KeyModifiers};
use curl::easy::Easy;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tui::backend::CrosstermBackend;
use tui::layout::{Alignment, Constraint, Direction, Layout};
use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Spans};
use tui::widgets::{
    self, Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Tabs, Widget, Wrap,
};
use tui::Terminal;

use crate::items::Crate;
use std::ops::Sub;

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
<Escape> | <C-r> focus the results block
<C-q> | <C-c> quit

# results mode
<C-s> focus the search bar
<j>, <k>, <up>, <down> move up and down the results
<h>, <l>, <left>, <right> move left and right between result tabs
<Enter> go to crate (browser)
<C-g> go to repository (browser)
<C-q> | <C-c> | <q> quit

"#;

/// Specifies current cursor location.
enum Mode {
    Search,
    Results,
}

const TAB_TITLES: [&str; 5] = ["Summary", "Readme", "Repository", "Stats", "Compare"];

/// List of result crate items.
#[derive(Default)]
struct Crates {
    /// List of crates
    items: Arc<Mutex<Vec<Crate>>>,
    /// Current state of the user-facing list interface
    state: ListState,
}

impl Crates {
    /// Creates a new `Crates` object using a list of `Crate` items.
    fn new(items: Vec<Crate>) -> Self {
        let items_arc = Arc::new(Mutex::new(items));
        let items_arc_clone = items_arc.clone();

        // spawn a new thread that will query
        //TODO currently super naive with sequential queries
        // using a global lock on the crate list
        //TODO
        std::thread::spawn(move || 'outer: loop {
            std::thread::sleep(Duration::from_millis(50));
            let mut items = items_arc_clone.lock().unwrap().clone();
            for (n, item) in &mut items.iter().enumerate() {
                if item.readme.is_none() && item.repository.is_some() {
                    // let repository_short = repository_url.splitn(2, "//").collect::<Vec<&str>>()[0].splitn
                    let repo_url = item.repository.clone().unwrap();
                    // println!("{}", repo_url);
                    let repo_short = format!(
                        "{}/{}",
                        repo_url.rsplitn(3, '/').collect::<Vec<&str>>()[1],
                        repo_url.rsplitn(3, '/').collect::<Vec<&str>>()[0]
                    );
                    // println!("{}", repo_short);

                    let mut buffer = vec![];
                    let mut handle = Easy::new();
                    let url = if repo_url.contains("github") {
                        format!(
                            "https://raw.githubusercontent.com/{}/master/README.md",
                            repo_short
                        )
                    } else if repo_url.contains("gitlab") {
                        format!("{}/raw/master/README.md", repo_url)
                    } else {
                        continue;
                    };

                    handle.url(&url);
                    {
                        let mut transfer = handle.transfer();
                        transfer.write_function(|data| {
                            buffer.extend_from_slice(data);
                            Ok(data.len())
                        });
                        transfer.perform();
                    }

                    // item.readme = Some(strip_markdown::strip_markdown(
                    //     &String::from_utf8(buffer.clone()).unwrap(),
                    // ));
                    items_arc_clone.lock().unwrap().get_mut(n).unwrap().readme = Some(
                        strip_markdown::strip_markdown(&String::from_utf8(buffer.clone()).unwrap()),
                    );

                    // println!("{:?}", item.readme);
                    // continue 'outer;
                }
            }
        });

        Crates {
            items: items_arc,
            state: ListState::default(),
        }
    }

    fn add(&mut self, item: Crate) {
        self.items.lock().unwrap().push(item);
    }

    fn select(&mut self, idx: Option<usize>) {
        if let Some(i) = idx {
            if i <= self.items.lock().unwrap().len() {
                self.state.select(idx);
            } else {
                // if let Some(select_target) = self.items.lock().unwrap().len().checked_sub(1) {
                //     self.state.select(&select_target);
                // }
                self.state
                    // .select(self.items.lock().unwrap().len().checked_sub(1))
                    .select(Some(self.items.lock().unwrap().len() - 1));
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
            _ => i.checked_sub(n.unwrap_or(1)).unwrap_or(0),
        }))
    }
}

/// Defines the main application loop.
fn main() -> Result<(), io::Error> {
    // set up tui using crossterm backend
    let stdout = io::stdout();
    crossterm::terminal::enable_raw_mode().unwrap();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut stdout = io::stdout();
    terminal.clear().unwrap();

    // create new crates.io client
    let client = SyncClient::new(
        "crate name search app (github.com/adamsky/cns)",
        std::time::Duration::from_millis(1000),
    )
    .unwrap();

    // initialize crate items list
    let mut crates = Crates::default();

    // start the application with the cursor on the search bar
    let mut current_mode = Mode::Search;
    // show application intro/help information
    let mut show_intro = true;

    // set up application interface blocks
    let mut search_block_title = "Search".to_string();
    let mut search_block_text = "".to_string();
    let mut results_block_label = "Results".to_string();
    let mut results_current_tab = 0;
    let mut search_block_border_style = Style::default().fg(tui::style::Color::DarkGray);
    let mut results_block_border_style = Style::default().fg(tui::style::Color::DarkGray);
    let mut results_block_highlight_style = Style::default().bg(tui::style::Color::DarkGray);

    // store some information on previously pressed keys to support basic
    // vim-like shortcuts like `gg`, along with num prefixed ones like `5j`
    let mut num_input = None;
    let mut last_key = KeyEvent {
        code: KeyCode::Null,
        modifiers: KeyModifiers::NONE,
    };
    let mut previous_key = KeyEvent {
        code: KeyCode::Null,
        modifiers: KeyModifiers::NONE,
    };

    // start main application loop
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

                // only used for enlarged results block when comparing crates
                let chunks_vert = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(3)
                    .constraints([Constraint::Length(3), Constraint::Min(10)].as_ref())
                    .split(f.size());

                let search_block = Block::default()
                    .title(search_block_title.as_str())
                    .borders(Borders::ALL)
                    .border_style(search_block_border_style);
                let search_block_text_final = match current_mode {
                    Mode::Search => format!("{}|", search_block_text),
                    _ => search_block_text.clone(),
                };
                let paragraph =
                    Paragraph::new(search_block_text_final.as_str()).block(search_block);
                f.render_widget(paragraph, chunks_left[0]);
                let items = crates.items.lock().unwrap();
                let mut list_items: Vec<ListItem> = items
                    .iter()
                    .map(|i| ListItem::new(i.name.as_str()))
                    .collect::<Vec<ListItem>>()
                    .clone();

                let mut rect = chunks_left[1];

                // some changes to results block are needed for the compare tab
                if results_current_tab == 4 {
                    rect = chunks_vert[1];
                    let comp_strings_titles =
                        vec!["downloads ".to_string(), "recent downloads ".to_string()];
                    let comp_strings_len: Vec<usize> =
                        comp_strings_titles.iter().map(|cs| cs.len()).collect();
                    // let comp_strings_len = vec!["downloads".len(), "recent downloads".len()];

                    let mut new_list_items = Vec::new();
                    for item in items.iter() {
                        let recent_downloads_string = match item.recent_downloads {
                            Some(s) => s.to_string(),
                            None => "n/a".to_string(),
                        };
                        let comp_strings =
                            vec![item.downloads.to_string(), recent_downloads_string];
                        let item_string = create_list_item_string(
                            item.name.to_string(),
                            comp_strings,
                            comp_strings_len.clone(),
                            ' ',
                            rect.width as usize,
                        );
                        let list_item = ListItem::new(Span::raw(item_string));
                        new_list_items.push(list_item);
                    }
                    list_items = new_list_items;

                    results_block_label.clear();
                    results_block_label = create_list_item_string(
                        "Results".to_string(),
                        comp_strings_titles,
                        comp_strings_len.clone(),
                        '─',
                        rect.width as usize,
                    );
                }

                let mut results_block = Block::default()
                    .title(results_block_label.as_str())
                    .borders(Borders::ALL)
                    .border_style(results_block_border_style);
                let results = List::new(list_items)
                    .block(results_block)
                    .highlight_style(results_block_highlight_style);

                f.render_stateful_widget(results, rect, &mut crates.state);

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
                    let titles = TAB_TITLES.iter().cloned().map(Spans::from).collect();
                    let top_tabs = Tabs::new(titles)
                        .select(results_current_tab)
                        .block(Block::default().title("").borders(Borders::BOTTOM))
                        .style(Style::default().fg(Color::DarkGray))
                        .highlight_style(Style::default().fg(Color::White));
                    // .divider(tui::symbols::);
                    f.render_widget(top_tabs, chunks_right[0]);

                    match results_current_tab {
                        0 => {
                            let summary = match crates.state.selected() {
                                Some(n) => {
                                    if let Some(item) = items.get(n) {
                                        format!(
                                            "\n\
                                            {}\n\n\
                                            {}\n\n\n\
                                            All-time: {}\n\
                                            Recent: {}\n\
                                            Last update: {}\n\
                                            First created: {}\n",
                                            item.name,
                                            item.description.as_ref().unwrap_or(&"".to_string()),
                                            item.downloads,
                                            item.recent_downloads.unwrap_or(0),
                                            item.updated_at,
                                            item.created_at
                                        )
                                    } else {
                                        "failed getting crate".to_string()
                                    }
                                }

                                None => "select a crate".to_string(),
                            };
                            f.render_widget(
                                widgets::Paragraph::new(summary.as_str())
                                    .wrap(Wrap { trim: false })
                                    .block(Block::default().borders(Borders::NONE)),
                                chunks_right[1],
                            );
                        }
                        1 => {
                            let readme = match crates.state.selected() {
                                Some(n) => {
                                    if let Some(item) = items.get(n) {
                                        item.readme
                                            .clone()
                                            .unwrap_or("(downloading...)".to_string())
                                    } else {
                                        "failed getting crate's readme".to_string()
                                    }
                                }
                                None => "select a crate".to_string(),
                            };
                            f.render_widget(
                                widgets::Paragraph::new(readme.as_str())
                                    .wrap(Wrap { trim: false })
                                    .block(Block::default().borders(Borders::NONE)),
                                chunks_right[1],
                            );
                        }
                        2 => {
                            f.render_widget(
                                widgets::Paragraph::new("2")
                                    .block(Block::default().borders(Borders::NONE)),
                                chunks_right[1],
                            );
                        }
                        3 => {
                            f.render_widget(
                                widgets::Paragraph::new("3")
                                    .block(Block::default().borders(Borders::NONE)),
                                chunks_right[1],
                            );
                        }
                        4 => {
                            // f.render_stateful_widget(results, chunks_horiz[0], &mut crates.state);
                            // f.render_widget(
                            //     widgets::Paragraph::new("3")
                            //         .block(Block::default().borders(Borders::NONE)),
                            //     chunks_right[1],
                            // );
                        }
                        _ => (),
                    }
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
                                KeyCode::Char('q') | KeyCode::Char('c') => break,
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
                                    crates = Crates::new(
                                        crate_query(&search_block_text, &client).unwrap(),
                                    );

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
                                KeyCode::Char('q') | KeyCode::Char('c') => break,
                                KeyCode::Char('s') => current_mode = Mode::Search,
                                KeyCode::Char('g') => {
                                    if let Some(selected_crate) = crates.state.selected() {
                                        if let Some(repo_url) = &crates
                                            .items
                                            .lock()
                                            .unwrap()
                                            .get(selected_crate)
                                            .unwrap()
                                            .repository
                                        {
                                            webbrowser::open(repo_url);
                                        }
                                    }
                                }

                                _ => continue,
                            }
                        } else {
                            match kc {
                                KeyCode::Esc => current_mode = Mode::Search,
                                KeyCode::Left | KeyCode::Char('h') => {
                                    if results_current_tab > 0 {
                                        results_current_tab -= 1
                                    }
                                }
                                KeyCode::Right | KeyCode::Char('l') => {
                                    if results_current_tab < TAB_TITLES.len() - 1 {
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
                                KeyCode::Enter => {
                                    if let Some(selected_crate) = crates.state.selected() {
                                        webbrowser::open(&format!(
                                            "https://crates.io/crates/{}",
                                            crates
                                                .items
                                                .lock()
                                                .unwrap()
                                                .get(selected_crate)
                                                .unwrap()
                                                .id
                                        ));
                                    }
                                }
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
                                    'G' => {
                                        let num = crates.items.lock().unwrap().len() - 1;
                                        crates.select(Some(num));
                                    }
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

/// Queries crates from the client using a simple string input.
fn crate_query(input: &str, client: &SyncClient) -> Result<Vec<Crate>, io::Error> {
    let opt = ListOptions {
        sort: Sort::Relevance,
        per_page: 50,
        page: 1,
        query: Some(input.to_string()),
    };
    let crates_response: CratesResponse = client.crates(opt).unwrap();

    let mut crates = Vec::new();
    for crate_response in &crates_response.crates {
        // println!("{:?}", crate_response);
        crates.push(Crate {
            id: crate_response.id.clone(),
            name: crate_response.name.clone(),
            description: crate_response.description.clone(),
            license: crate_response.license.clone(),
            documentation: crate_response.documentation.clone(),
            homepage: crate_response.homepage.clone(),
            repository: crate_response.repository.clone(),
            downloads: crate_response.downloads,
            recent_downloads: crate_response.recent_downloads.clone(),
            categories: crate_response.categories.clone(),
            keywords: crate_response.keywords.clone(),
            max_version: crate_response.max_version.clone(),
            links: crate_response.links.clone(),
            created_at: crate_response.created_at.to_rfc2822(),
            updated_at: crate_response.updated_at.to_rfc2822(),
            exact_match: crate_response.exact_match,
            readme: None,
        })
    }

    Ok(crates)

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

fn create_list_item_string(
    left_string: String,
    right_strings: Vec<String>,
    right_strings_width: Vec<usize>,
    space_char: char,
    rect_width: usize,
) -> String {
    let mut item_string = left_string.to_string();
    let mut left_right_delta = rect_width - left_string.len();
    for right_string_width in &right_strings_width {
        left_right_delta = left_right_delta.sub(right_string_width + 4);
    }

    for _ in 0..left_right_delta {
        item_string.push(space_char);
    }

    for (i, right_string) in right_strings.iter().enumerate() {
        let width_delta = right_strings_width[i] - right_string.len();
        let rs = format!("| {}", right_string);
        item_string.push_str(&rs);
        for _ in 0..width_delta {
            item_string.push(space_char);
        }
    }

    item_string
}
