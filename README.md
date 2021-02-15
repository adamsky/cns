# crate name search

Search through Rust crates without leaving the terminal.

Get a quick summary, compare different crates, quickly open crate
repositories and documentation in the browser, copy `Cargo.toml` dependency
lines to clipboard, and more. 

![](.github/cns_demo.gif)

`cns` supports composite queries of categories, keywords, strings, and sorts:
```text
# get sudoku games with most recent downloads
cat=games sudoku sort=rdl
# see what's new in the web development space
key=web sort=new
# look for actively maintained socket programming libraries
socket sort=update
# see the most popular crate search apps
key=crates search sort=dl 
```


## How to install `cns`

```
cargo install cns
```

or

```
git clone https://github.com/adamsky/cns
cd ./cns
cargo run --release
```


## How to use `cns`

```
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
```  