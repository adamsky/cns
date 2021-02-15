#![allow(unused)]

use std::io::{self, Write};
use std::ops::Sub;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use consecrates::api::{CrateResponse, Crates};
use consecrates::Client;

use anyhow::Result;
use chrono::{DateTime, Utc};
use clipboard::ClipboardProvider;
use crossterm::event::{read, Event, KeyCode, KeyEvent, KeyModifiers};
use http_req::uri::Uri;
use items::Crate;
use tui::backend::CrosstermBackend;
use tui::layout::{Alignment, Constraint, Direction, Layout};
use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Spans};
use tui::widgets::{
    self, Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Tabs, Widget, Wrap,
};
use tui::Terminal;

mod items;

pub const INTRO: &str = r#"
                  __
.----.----.---.-.|  |_.-----.
|  __|   _|  _  ||   _|  -__|
|____|__| |___._||____|_____|
.-----.---.-.--------.-----.
|     |  _  |        |  -__|
|__|__|___._|__|__|__|_____| v0.1.1
.-----.-----.---.-.----.----.|  |--.
|__ --|  -__|  _  |   _|  __||     |
|_____|_____|___._|__| |____||__|__|

<C-h> toggle help window 


<recent>

<just>

<new>
"#;

pub const HELP: &str = r#"
                  __
.----.----.---.-.|  |_.-----.
|  __|   _|  _  ||   _|  -__|
|____|__| |___._||____|_____|
.-----.---.-.--------.-----.
|     |  _  |        |  -__|
|__|__|___._|__|__|__|_____| v0.1.1
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
<Escape> | <C-s> focus the search bar
<k>, <j>, <up>, <down> move up and down the results
<h>, <l>, <left>, <right> move left and right between result tabs
<C-u>, <C-d> scroll up and down the readme view
<C-g> go to documentation (browser)
<C-r> go to repository (browser)
<Enter> go to crate (browser)
<c> copy Cargo.toml dependency line to clipboard
<x> copy clone+compile+run one-liner to clipboard
<C-q> | <C-c> | <q> quit

"#;

pub const README_SCROLL_AMOUNT: u16 = 8;

/// Specifies current cursor location.
enum Mode {
    Search,
    Results,
}

enum InfoScreen {
    Intro,
    Help,
}

const TAB_TITLES: [&str; 5] = ["Summary", "Compare", "Readme", "Repository", "Stats"];

/// List of crate items.
///
/// # State
///
/// This struct holds both the crate structs and the current state of the
/// TUI list.
#[derive(Default)]
struct CratesList {
    /// List of crates
    items: Arc<Mutex<Vec<Crate>>>,
    /// Current state of the user-facing list interface
    list_state: ListState,
    /// Current vertical offset of the readme viewport
    readme_scroll: u16,
}

impl CratesList {
    /// Creates a new `Crates` object using a list of `Crate` items.
    fn new(items: Vec<Crate>) -> Self {
        let items_arc = Arc::new(Mutex::new(items));
        let items_arc_clone = items_arc.clone();

        // spawn a new thread that will query crates' readmes
        //TODO this is a quick and dirty approach with sequential queries
        // using a global lock on the crate list
        //TODO find a better way to get crate readmes
        std::thread::spawn(move || 'outer: loop {
            std::thread::sleep(Duration::from_millis(100));
            let mut items = items_arc_clone.lock().unwrap().clone();
            for (n, item) in &mut items.iter().enumerate() {
                if item.readme.is_none() && item.repository.is_some() {
                    let repo_url = item.repository.clone().unwrap();
                    let repo_short = format!(
                        "{}/{}",
                        repo_url.rsplitn(3, '/').collect::<Vec<&str>>()[1],
                        repo_url.rsplitn(3, '/').collect::<Vec<&str>>()[0]
                    );

                    // this only works for github/gitlab repos with master branch
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

                    let mut buffer = vec![];
                    if let Ok(resp) = http_req::request::get(url, &mut buffer) {
                        if let Ok(s) = String::from_utf8(buffer) {
                            items_arc_clone.lock().unwrap().get_mut(n).unwrap().readme = Some(s);
                        }
                    }
                }
            }
        });

        CratesList {
            items: items_arc,
            list_state: ListState::default(),
            readme_scroll: 0,
        }
    }

    /// Adds crate item to the collection.
    fn add(&mut self, item: Crate) {
        self.items.lock().unwrap().push(item);
    }

    /// Selects crate in the collection based on the given index.
    /// If index is `None` deselects the current selection.
    fn select(&mut self, idx: Option<usize>) {
        if let Some(i) = idx {
            if i < self.items.lock().unwrap().len() {
                self.list_state.select(idx);
            } else {
                self.list_state
                    .select(self.items.lock().unwrap().len().checked_sub(1));
            }
        } else {
            self.list_state.select(idx);
        }

        // reset the readme scroll on change to current selection
        self.readme_scroll = 0;
    }

    /// Selects next crate in the collection.
    fn select_next(&mut self, n: Option<usize>) {
        self.select(self.list_state.selected().map(|i| i + n.unwrap_or(1)));
    }

    /// Selects previous crate in the collection.
    fn select_previous(&mut self, n: Option<usize>) {
        self.select(self.list_state.selected().map(|i| match i {
            0 => 0,
            _ => i.checked_sub(n.unwrap_or(1)).unwrap_or(0),
        }))
    }
}

/// Defines the main application loop.
fn main() -> Result<()> {
    let mut get_summary = true;
    let mut args = std::env::args();
    if args.find(|a| a == "--no-summary").is_some() {
        get_summary = false;
    }

    #[cfg(feature = "clipboard")]
    let mut clipboard = clipboard::ClipboardContext::new().unwrap();

    // set up tui using crossterm backend
    let stdout = io::stdout();
    crossterm::terminal::enable_raw_mode().unwrap();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut stdout = io::stdout();
    terminal.clear()?;

    // create new crates.io client
    let client = Client::new("crate_name_search (github.com/adamsky/cns)");

    let mut intro_string = HELP.to_string();
    // load up the registry summary data
    if get_summary {
        let summary = client.get_registry_summary()?;
        intro_string = create_intro_string(summary)?;
    }

    // initialize crate items list
    let mut crates = CratesList::default();

    // start the application with the cursor on the search bar
    let mut current_mode = Mode::Search;
    // intro/help information screen toggle
    let mut show_info = Some(InfoScreen::Intro);

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
                if results_current_tab == 1 && show_info.is_none() {
                    rect = chunks_vert[1];
                    let comp_strings_titles = vec![
                        "Since creation ".to_string(),
                        "Since update ".to_string(),
                        "All-time dl ".to_string(),
                        "Recent dl ".to_string(),
                        "Max version ".to_string(),
                        "Repo host ".to_string(),
                    ];
                    let comp_strings_len: Vec<usize> =
                        comp_strings_titles.iter().map(|cs| cs.len()).collect();

                    let mut new_list_items = Vec::new();
                    for item in items.iter() {
                        let recent_downloads_string = match item.recent_downloads {
                            Some(s) => s.to_string(),
                            None => "n/a".to_string(),
                        };
                        let days_since_creation =
                            Utc::now().sub(item.created_at).num_days().to_string();
                        let days_since_update =
                            Utc::now().sub(item.updated_at).num_days().to_string();
                        let max_version = item.max_version.clone();
                        let mut repo_host = "n/a".to_string();

                        if let Some(repo_url) = &item.repository {
                            if let Ok(uri) = repo_url.parse::<Uri>() {
                                if let Some(host) = uri.host() {
                                    repo_host = host.to_string();
                                }
                            }
                        }

                        let comp_strings = vec![
                            days_since_creation,
                            days_since_update,
                            item.downloads.to_string(),
                            recent_downloads_string,
                            max_version,
                            repo_host,
                        ];
                        let item_string = create_list_item_string(
                            item.name.to_string(),
                            comp_strings,
                            comp_strings_len.clone(),
                            ' ',
                            '|',
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
                        '─',
                        rect.width as usize,
                    );
                } else {
                    results_block_label = "Results".to_string();
                }

                let mut results_block = Block::default()
                    .title(results_block_label.as_str())
                    .borders(Borders::ALL)
                    .border_style(results_block_border_style);
                let results = List::new(list_items)
                    .block(results_block)
                    .highlight_style(results_block_highlight_style);

                f.render_stateful_widget(results, rect, &mut crates.list_state);

                if let Some(info) = &show_info {
                    match info {
                        InfoScreen::Help => {
                            let info = widgets::Paragraph::new(HELP)
                                .wrap(Wrap { trim: false })
                                .block(Block::default().borders(Borders::NONE));
                            f.render_widget(info, chunks_horiz[2]);
                        }
                        InfoScreen::Intro => {
                            let info = widgets::Paragraph::new(intro_string.clone())
                                .wrap(Wrap { trim: false })
                                .block(Block::default().borders(Borders::NONE));
                            f.render_widget(info, chunks_horiz[2]);
                        }
                    }
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
                    f.render_widget(top_tabs, chunks_right[0]);

                    match results_current_tab {
                        0 => {
                            let summary = match crates.list_state.selected() {
                                Some(n) => {
                                    if let Some(item) = items.get(n) {
                                        format!(
                                            "{}\n\n\
                                            {}\n\n\n\
                                            Max version: {}\n\
                                            Homepage: {}\n\n\
                                            All-time downloads: {}\n\
                                            Recent downloads: {}\n\
                                            Days since last update: {}\n\
                                            \n\
                                            First created: {}\n\
                                            Last update: {}\n\
                                            \n\
                                            Documentation: {}\n\
                                            Repository: {}\n",
                                            item.name,
                                            item.description.as_ref().unwrap_or(&"".to_string()),
                                            item.max_version,
                                            item.homepage.clone().unwrap_or("n/a".to_string()),
                                            item.downloads,
                                            item.recent_downloads.unwrap_or(0),
                                            Utc::now().sub(item.updated_at).num_days(),
                                            item.created_at,
                                            item.updated_at,
                                            item.documentation
                                                .as_ref()
                                                .unwrap_or(&"unavailable".to_string()),
                                            item.repository
                                                .as_ref()
                                                .unwrap_or(&"unavailable".to_string())
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
                            // compare tab renders a wider results block
                        }
                        2 => {
                            let readme = match crates.list_state.selected() {
                                Some(n) => {
                                    if let Some(item) = items.get(n) {
                                        item.readme
                                            .clone()
                                            .unwrap_or("(downloading...)".to_string())
                                    } else {
                                        "failed getting crate".to_string()
                                    }
                                }
                                None => "select a crate".to_string(),
                            };
                            f.render_widget(
                                widgets::Paragraph::new(readme.as_str())
                                    .scroll((crates.readme_scroll, 0))
                                    .wrap(Wrap { trim: false })
                                    .block(Block::default().borders(Borders::NONE)),
                                chunks_right[1],
                            );
                        }
                        3 => {
                            f.render_widget(
                                widgets::Paragraph::new("WIP")
                                    .block(Block::default().borders(Borders::NONE)),
                                chunks_right[1],
                            );
                        }
                        4 => {
                            f.render_widget(
                                widgets::Paragraph::new("WIP")
                                    .block(Block::default().borders(Borders::NONE)),
                                chunks_right[1],
                            );
                        }
                        _ => (),
                    }
                }
            })
            .unwrap();

        if let Event::Key(key_event) = read().unwrap() {
            previous_key = last_key;
            last_key = key_event.clone();

            match current_mode {
                // bindings for when the cursor is focused on search
                Mode::Search => {
                    if key_event.modifiers == KeyModifiers::CONTROL {
                        match key_event.code {
                            KeyCode::Char('h') => {
                                show_info = match show_info {
                                    Some(info) => match info {
                                        InfoScreen::Intro => Some(InfoScreen::Help),
                                        _ => None,
                                    },
                                    None => Some(InfoScreen::Help),
                                }
                            }
                            KeyCode::Char('j') => {
                                show_info = match show_info {
                                    Some(info) => match info {
                                        InfoScreen::Help => Some(InfoScreen::Intro),
                                        _ => None,
                                    },
                                    None => Some(InfoScreen::Intro),
                                }
                            }
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
                        match key_event.code {
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
                                crates = CratesList::new(
                                    crate_query(&search_block_text, &client).unwrap(),
                                );

                                crates.select(Some(0));
                                show_info = None;
                                current_mode = Mode::Results;
                            }
                            _ => (),
                        }
                    }
                }
                // bindings for when cursor is focused on the results
                Mode::Results => {
                    // bindings with the ctrl key
                    if key_event.modifiers == KeyModifiers::CONTROL {
                        match key_event.code {
                            // show intro with bindings help
                            KeyCode::Char('h') => {
                                show_info = match show_info {
                                    Some(info) => match info {
                                        InfoScreen::Intro => Some(InfoScreen::Help),
                                        _ => None,
                                    },
                                    None => Some(InfoScreen::Help),
                                }
                            }
                            KeyCode::Char('j') => {
                                show_info = match show_info {
                                    Some(info) => match info {
                                        InfoScreen::Help => Some(InfoScreen::Intro),
                                        _ => None,
                                    },
                                    None => Some(InfoScreen::Intro),
                                }
                            }
                            // focus the search mode
                            KeyCode::Char('s') => current_mode = Mode::Search,
                            // open crate repository in the browser
                            KeyCode::Char('r') => {
                                if let Some(selected_crate) = crates.list_state.selected() {
                                    if let Some(url) = &crates
                                        .items
                                        .lock()
                                        .unwrap()
                                        .get(selected_crate)
                                        .unwrap()
                                        .repository
                                    {
                                        webbrowser::open(url);
                                    }
                                }
                            }
                            KeyCode::Char('g') => {
                                if let Some(selected_crate) = crates.list_state.selected() {
                                    if let Some(url) = &crates
                                        .items
                                        .lock()
                                        .unwrap()
                                        .get(selected_crate)
                                        .unwrap()
                                        .documentation
                                    {
                                        webbrowser::open(url);
                                    }
                                }
                            }
                            KeyCode::Char('d') => {
                                if results_current_tab == 2 {
                                    crates.readme_scroll += README_SCROLL_AMOUNT;
                                }
                            }
                            KeyCode::Char('u') => {
                                if results_current_tab == 2 {
                                    let mut sub = crates.readme_scroll as isize
                                        - README_SCROLL_AMOUNT as isize;
                                    if sub < 0 {
                                        sub = 0;
                                    }
                                    crates.readme_scroll = sub as u16;
                                }
                            }
                            // quit the application altogether
                            KeyCode::Char('q') | KeyCode::Char('c') => break,

                            _ => continue,
                        }
                    } else {
                        match key_event.code {
                            // focus the search mode
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
                            // open crate page in the browser
                            KeyCode::Enter => {
                                if let Some(selected_crate) = crates.list_state.selected() {
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
                                #[cfg(feature = "clipboard")]
                                'c' => {
                                    if let Some(selection) = crates.list_state.selected() {
                                        if let Some(sel_crate) =
                                            crates.items.lock().unwrap().get(selection)
                                        {
                                            let clip_text = format!(
                                                "{} = \"{}\"",
                                                sel_crate.id, sel_crate.max_version
                                            );
                                            clipboard.set_contents(clip_text);
                                        }
                                    }
                                }
                                #[cfg(feature = "clipboard")]
                                'x' => {
                                    if let Some(selection) = crates.list_state.selected() {
                                        if let Some(sel_crate) =
                                            crates.items.lock().unwrap().get(selection)
                                        {
                                            if let Some(repo) = &sel_crate.repository {
                                                let uri: Uri = repo.parse()?;
                                                let repo_name = uri
                                                    .path()
                                                    .unwrap()
                                                    .rsplit('/')
                                                    .collect::<Vec<&str>>()[0];
                                                let clip_text = format!(
                                                    "git clone {} && cd {} && cargo run --release",
                                                    repo, repo_name
                                                );
                                                clipboard.set_contents(clip_text);
                                            }
                                        }
                                    }
                                }
                                // quit the application
                                'q' => break,
                                _ => (),
                            },

                            _ => (),
                        }
                    }
                }
            }
        }
        // reset combos
        num_input = None;

        stdout.flush().unwrap();
    }

    // clean up the terminal before exit
    terminal.clear().unwrap();
    Ok(())
}

/// Queries crates from the client using a simple string input.
fn crate_query(input: &str, client: &Client) -> Result<Vec<Crate>> {
    let mut query = consecrates::Query::from_str(input);
    let crates_response: Crates = client.get_crates(query)?;

    let mut crates = Vec::new();
    for crate_response in &crates_response.crates {
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
            created_at: crate_response.created_at,
            updated_at: crate_response.updated_at,
            exact_match: crate_response.exact_match,
            readme: None,
        })
    }

    Ok(crates)
}

/// Creates a new results list item string using a bunch of arguments.
///
/// Organizes text into a left and right column, where the right column
/// consists of zero or more elements.
fn create_list_item_string(
    left_string: String,
    right_strings: Vec<String>,
    right_strings_width: Vec<usize>,
    space_char: char,
    div_char: char,
    rect_width: usize,
) -> String {
    let mut item_string = left_string.to_string();

    // calculate space between the end of the left and beginning of the right
    let mut left_right_delta = rect_width - left_string.len();
    for right_string_width in &right_strings_width {
        left_right_delta = left_right_delta
            .checked_sub(right_string_width + 4)
            .unwrap_or(0);
    }

    // push the right amount of space characters
    for _ in 0..left_right_delta {
        item_string.push(space_char);
    }

    // push all the right column strings
    for (i, right_string) in right_strings.iter().enumerate() {
        if let Some(width_delta) = right_strings_width[i].checked_sub(right_string.len()) {
            let rs = format!("{} {}", div_char, right_string);
            item_string.push_str(&rs);
            for _ in 0..width_delta {
                item_string.push(space_char);
            }
        }
    }

    item_string
}

fn create_intro_string(summary: consecrates::api::Summary) -> Result<String> {
    let mut intro = INTRO.to_string();
    let mut recent = format!(
        r#"most recent downloads:
    {} | {} | {} | {}"#,
        summary.most_recently_downloaded[0].name,
        summary.most_recently_downloaded[1].name,
        summary.most_recently_downloaded[2].name,
        summary.most_recently_downloaded[3].name
    );
    let mut just = format!(
        r#"just updated:
    {} | {} | {} | {}"#,
        summary.just_updated[0].name,
        summary.just_updated[1].name,
        summary.just_updated[2].name,
        summary.just_updated[3].name
    );
    let mut new = format!(
        r#"new crates:
    {} | {} | {} | {}"#,
        summary.new_crates[0].name,
        summary.new_crates[1].name,
        summary.new_crates[2].name,
        summary.new_crates[3].name
    );

    intro = intro.replace("<recent>", &recent);
    intro = intro.replace("<just>", &just);
    intro = intro.replace("<new>", &new);

    Ok(intro)
}
