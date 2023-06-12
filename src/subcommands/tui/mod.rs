mod state;
mod util;
mod widgets;

use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use ckb_util::RwLock;
use termion::event::Key;
use termion::input::MouseTerminal;
use termion::raw::IntoRawMode;
use termion::screen::AlternateScreen;
use tui::backend::{Backend, TermionBackend};
use tui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
// use tui::widgets::canvas::{Canvas, Map, MapResolution, Rectangle};
// use tui::widgets::{Dataset, Axis, Chart, Marker};
use tui::widgets::{Block, Borders, Paragraph, SelectableList, Text, Widget};
use tui::{Frame, Terminal};
// use chrono::{Local, DateTime, TimeZone};
use ckb_index::{with_index_db, IndexDatabase};
use ckb_sdk::{constants::ONE_CKB, Address, GenesisInfo, HttpRpcClient, NetworkType};
use ckb_types::{
    core::{service::Request, BlockView},
    prelude::*,
    H256,
};

use crate::utils::{
    index::{IndexController, IndexRequest},
    other::get_network_type,
};
use state::{start_rpc_thread, State, SummaryInfo};
use util::{human_capacity, ts_now, App, Event, Events, TabsState};
use widgets::List;

pub struct TuiSubCommand {
    url: String,
    index_dir: PathBuf,
    index_controller: IndexController,
}

impl TuiSubCommand {
    pub fn new(
        url: String,
        index_dir: PathBuf,
        index_controller: IndexController,
    ) -> TuiSubCommand {
        TuiSubCommand {
            url,
            index_dir,
            index_controller,
        }
    }

    pub fn start(self) -> Result<String, String> {
        let (network_type, genesis_info) = {
            let mut rpc_client = HttpRpcClient::new(self.url.clone());
            let network_type = get_network_type(&mut rpc_client)?;
            let genesis_block: BlockView = rpc_client
                .get_block_by_number(0)
                .map_err(|err| {
                    format!(
                        "Get genesis block from {} failed: {}\n[Hint]: you can use `ckb-cli --url <URL> tui` to override API url",
                        self.url,
                        err
                    )
                })?
                .expect("Can not get genesis block?")
                .into();
            let genesis_info = GenesisInfo::from_block(&genesis_block)?;
            (network_type, genesis_info)
        };

        let stdout = io::stdout()
            .into_raw_mode()
            .map_err(|err| err.to_string())?;
        let stdout = MouseTerminal::from(stdout);
        let stdout = AlternateScreen::from(stdout);
        let backend = TermionBackend::new(stdout);
        let mut terminal = Terminal::new(backend).map_err(|err| err.to_string())?;
        terminal.hide_cursor().map_err(|err| err.to_string())?;

        let events = Events::new();
        let state = Arc::new(RwLock::new(State::default()));
        Request::call(
            self.index_controller.sender(),
            IndexRequest::UpdateUrl(self.url.clone()),
        );
        start_rpc_thread(self.url.clone(), Arc::clone(&state));
        // App
        let mut app = App {
            menu_active: true,
            tabs: TabsState::new(vec!["Summary", "Recent Blocks", "Peers", "Top Capacity"]),
        };

        // Main loop
        loop {
            terminal
                .draw(|mut f| {
                    let size = f.size();
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
                        .split(size);
                    let body_chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Length(17), Constraint::Min(1)].as_ref())
                        .split(chunks[1]);

                    let banner_context = RenderContext {
                        block: Block::default().borders(Borders::ALL),
                        frame: &mut f,
                        rect: chunks[0],
                    };
                    render_bannar(&state.read(), banner_context);

                    let menu_context = RenderContext {
                        block: Block::default().borders(Borders::ALL),
                        frame: &mut f,
                        rect: body_chunks[0],
                    };
                    render_menu(&app, menu_context);

                    // Content
                    let mut content_block = Block::default()
                        .title(app.tabs.titles[app.tabs.index].trim())
                        .borders(Borders::ALL);
                    if !app.menu_active {
                        content_block = content_block
                            .border_style(Style::default().fg(Color::Green))
                            .title_style(Style::default().modifier(Modifier::BOLD));
                    }
                    let mut content_context = RenderContext {
                        block: content_block,
                        frame: &mut f,
                        rect: body_chunks[1],
                    };
                    match app.tabs.index {
                        0 => render_summary(&state.read(), self.url.as_str(), content_context),
                        1 => render_blocks(&state.read(), content_context),
                        2 => render_peers(&state.read(), content_context),
                        3 => {
                            let title = format!(
                                "{} ({})",
                                app.tabs.titles[app.tabs.index].trim(),
                                self.index_controller.state().read(),
                            );
                            content_context.block =
                                Block::default().title(&title).borders(Borders::ALL);
                            render_top_capacity(
                                &self.index_controller,
                                self.index_dir.clone(),
                                network_type,
                                &genesis_info,
                                content_context,
                            )
                        }
                        _ => {}
                    }
                })
                .map_err(|err| err.to_string())?;

            match events.next().map_err(|err| err.to_string())? {
                Event::Input(input) => match input {
                    Key::Char('q') => {
                        break;
                    }
                    Key::Left | Key::Char('h') => {
                        app.menu_active = true;
                    }
                    Key::Right | Key::Char('l') => {
                        app.menu_active = false;
                    }
                    Key::Down | Key::Char('j') => {
                        if app.menu_active {
                            app.tabs.next();
                        }
                    }
                    Key::Up | Key::Char('k') => {
                        if app.menu_active {
                            app.tabs.previous();
                        }
                    }
                    _ => {}
                },
                Event::Tick => {}
            }
        }
        Ok("".to_owned())
    }
}

struct RenderContext<'a, 'b, B: Backend> {
    block: Block<'a>,
    frame: &'a mut Frame<'b, B>,
    rect: Rect,
}

fn render_bannar<B: Backend>(state: &State, ctx: RenderContext<B>) {
    let chain_name = state
        .chain
        .as_ref()
        .map(|info| info.chain.to_string())
        .unwrap_or_else(|| "<unknown>".to_string());
    let version = state
        .local_node
        .as_ref()
        .map(|info| info.version.to_string())
        .unwrap_or_else(|| "<unknown>".to_string());
    let texts = [
        Text::raw(" <"),
        Text::styled(chain_name, Style::default().fg(Color::Green)),
        Text::raw("> "),
        Text::styled(
            "CKB",
            Style::default().fg(Color::Blue).modifier(Modifier::BOLD),
        ),
        Text::raw(" "),
        Text::raw(version),
    ];
    Paragraph::new(texts.iter())
        .block(ctx.block)
        .alignment(Alignment::Left)
        .render(ctx.frame, ctx.rect);
}

fn render_menu<B: Backend>(app: &App, ctx: RenderContext<B>) {
    let menu_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(6)].as_ref())
        .split(ctx.rect);

    // Menu
    let mut menu_block = ctx.block.title("Menu");
    let mut highlight_style = Style::default().fg(Color::Black).modifier(Modifier::BOLD);
    if app.menu_active {
        menu_block = menu_block
            .border_style(Style::default().fg(Color::Green))
            .title_style(Style::default().modifier(Modifier::BOLD));
        highlight_style = highlight_style.bg(Color::LightYellow);
    }
    SelectableList::default()
        .block(menu_block)
        .items(&app.tabs.fixed_titles())
        .select(Some(app.tabs.index))
        .highlight_style(highlight_style)
        .render(ctx.frame, menu_chunks[0]);

    // Menu doc
    let docs = vec![
        Text::raw("\n"),
        Text::styled("Quit ", Style::default().modifier(Modifier::BOLD)),
        Text::raw(": Q"),
        Text::raw("\n"),
        Text::styled("Help ", Style::default().modifier(Modifier::BOLD)),
        Text::raw(": ?"),
        Text::raw("\n"),
    ];
    Paragraph::new(docs.iter())
        .block(Block::default().title("Help").borders(Borders::ALL))
        .alignment(Alignment::Center)
        .render(ctx.frame, menu_chunks[1]);
}

fn render_summary<B: Backend>(state: &State, url: &str, ctx: RenderContext<B>) {
    let SummaryInfo {
        chain,
        tip,
        tx_pool,
        peer_count,
    } = state.summary();
    let mut length: u16 = 0;
    let mut lines = vec![];
    let mut push_pair = |name: &str, content_opt: Option<String>, style_opt: Option<Style>| {
        length += 1;
        lines.push(Text::styled(
            format!("{} ", name),
            Style::default().modifier(Modifier::BOLD),
        ));

        let content = content_opt.unwrap_or_else(|| "<unknown>".to_string());
        if let Some(style) = style_opt {
            lines.push(Text::raw(": "));
            lines.push(Text::styled(content, style));
        } else {
            lines.push(Text::raw(format!(": {}", content)));
        }
        lines.push(Text::raw("\n"));
    };

    let chain_name = chain.as_ref().map(|info| info.chain.to_string());
    let epoch = chain.as_ref().map(|info| info.epoch.to_string());
    let difficulty = chain.as_ref().map(|info| info.difficulty.to_string());
    let ibd = chain
        .as_ref()
        .map(|info| info.is_initial_block_download.to_string());
    // TODO: query alert instead
    let warnings = None;
    // let warnings = chain.as_ref().and_then(|info| {
    //     if info.warnings.is_empty() {
    //         None
    //     } else {
    //         Some(info.warnings.to_string())
    //     }
    // });
    let tip_info = tip
        .as_ref()
        .map(|block| format!("{} => {}", block.header.number(), block.header.hash()));
    let tx_pool_info = tx_pool.map(|info| {
        format!(
            "pending={}, proposed={}, orphan={}",
            info.pending, info.proposed, info.orphan,
        )
    });
    let peers_count = Some(format!("{}", peer_count));

    let tip_style = tip.as_ref().and_then(|block| {
        if ts_now().saturating_sub(block.got_at) < 2000 {
            Some(Style::default().fg(Color::Black).bg(Color::LightBlue))
        } else {
            None
        }
    });
    let warn_style = Style::default().fg(Color::Yellow).modifier(Modifier::BOLD);
    push_pair(" API URL   ", Some(url.to_owned()), None);
    push_pair(" Chain     ", chain_name, None);
    push_pair(" Epoch     ", epoch, None);
    push_pair(" Difficulty", difficulty, None);
    push_pair(" IBD       ", ibd, None);
    push_pair(" Tip Block ", tip_info, tip_style);
    push_pair(" TxPool    ", tx_pool_info, None);
    push_pair(" Peers     ", peers_count, None);
    if warnings.is_some() {
        push_pair(" Warnings  ", warnings, Some(warn_style));
    }

    ctx.block.clone().render(ctx.frame, ctx.rect);
    let summary_chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints(
            [
                Constraint::Length(length),
                Constraint::Length(2),
                Constraint::Min(2),
            ]
            .as_ref(),
        )
        .split(ctx.rect);
    Paragraph::new(lines.iter())
        .alignment(Alignment::Left)
        .render(ctx.frame, summary_chunks[0]);

    // let now = Local::now();
    // let five_minutes_ago = now - chrono::Duration::minutes(5);
    // let ten_minutes_ago = now - chrono::Duration::minutes(10);
    // let blocks = state.blocks
    //     .iter()
    //     .filter(|(_, block)| {
    //         block.header.timestamp() >= ten_minutes_ago.timestamp_millis() as u64
    //     })
    //     .collect::<BTreeMap<_, _>>();
    // let min_number = blocks.keys().next().map(|n| **n).unwrap_or(0);
    // let max_number = blocks.keys().rev().next().map(|n| **n).unwrap_or(0);
    // let mid_number = (min_number + max_number) / 2;
    // let heights = blocks
    //     .iter()
    //     .filter(|(_, block)| {
    //         block.header.timestamp() >= ten_minutes_ago.timestamp_millis() as u64
    //     })
    //     .map(|(number, block)| {
    //         ((block.header.timestamp() / 1000) as f64, **number as f64)
    //     })
    //     .collect::<Vec<_>>();
    // Chart::default()
    //     .block(Block::default().borders(Borders::ALL))
    //     .x_axis(
    //         Axis::default()
    //             .title("Time")
    //             .style(Style::default().fg(Color::Gray))
    //             .bounds([
    //                 (ten_minutes_ago.timestamp_millis() / 1000) as f64,
    //                 (now.timestamp_millis() / 1000) as f64,
    //             ])
    //             .labels(&[
    //                 &format!("{}", ten_minutes_ago.format("%H:%M:%S").to_string()),
    //                 &format!("{}", five_minutes_ago.format("%H:%M:%S").to_string()),
    //                 &format!("{}", now.format("%H:%M:%S").to_string()),
    //             ]),
    //     )
    //     .y_axis(
    //         Axis::default()
    //             .title("Height")
    //             .style(Style::default().fg(Color::Gray))
    //             .bounds([min_number as f64, max_number as f64])
    //             .labels(&[min_number.to_string(), mid_number.to_string(), max_number.to_string()]),
    //     )
    //     .datasets(&[
    //         Dataset::default()
    //             .marker(Marker::Dot)
    //             .style(Style::default().fg(Color::Yellow))
    //             .data(&heights),
    //     ])
    //     .render(ctx.frame, summary_chunks[2]);

    // Canvas::default()
    //     .paint(|ctx| {
    //         ctx.draw(&Map {
    //             color: Color::White,
    //             resolution: MapResolution::High,
    //         });
    //         ctx.print(0.0, 0.0, "x", Color::Yellow);
    //     })
    //     .x_bounds([-180.0, 180.0])
    //     .y_bounds([-90.0, 90.0])
    //     .render(ctx.frame, summary_chunks[2]);
}

fn render_blocks<B: Backend>(state: &State, ctx: RenderContext<B>) {
    let blocks = state.blocks.values().rev().flat_map(|block| {
        let header = &block.header;
        let block_hash: H256 = header.hash().unpack();
        vec![
            Text::styled(
                format!("{} => {:x}", header.number(), block_hash),
                Style::default().modifier(Modifier::BOLD),
            ),
            Text::raw(format!(
                "  commited={}, proposed={}, uncles={}, inputs={}, outputs={}, cellbase={}",
                block.commit_tx_count,
                block.proposal_tx_count,
                block.uncle_count,
                block.input_count,
                block.output_count,
                block
                    .cellbase_outputs
                    .iter()
                    .map(|(capacity, _)| *capacity)
                    .sum::<u64>(),
            )),
        ]
    });

    ctx.block.clone().render(ctx.frame, ctx.rect);
    let blocks_chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([Constraint::Percentage(100)].as_ref())
        .split(ctx.rect);
    List::new(blocks).render(ctx.frame, blocks_chunks[0]);
}

fn render_peers<B: Backend>(state: &State, ctx: RenderContext<B>) {
    let max_width = state
        .peers
        .iter()
        .filter_map(|node| node.addresses.get(0).map(|addr| addr.address.len()))
        .max()
        .unwrap_or(10);
    let peers = state.peers.iter().flat_map(|node| {
        let direction = if node.is_outbound {
            "outbound"
        } else {
            "inbound"
        };
        vec![Text::raw(format!(
            "{:<width$} {:8} version({})",
            node.addresses
                .get(0)
                .map(|addr| addr.address.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            direction,
            node.version,
            width = max_width,
        ))]
    });

    ctx.block.clone().render(ctx.frame, ctx.rect);
    let peers_chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([Constraint::Percentage(100)].as_ref())
        .split(ctx.rect);
    List::new(peers).render(ctx.frame, peers_chunks[0]);
}

fn render_top_capacity<B: Backend>(
    index: &IndexController,
    index_dir: PathBuf,
    network_type: NetworkType,
    genesis_info: &GenesisInfo,
    ctx: RenderContext<B>,
) {
    ctx.block.clone().render(ctx.frame, ctx.rect);
    let top_capacity_chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([Constraint::Percentage(100)].as_ref())
        .split(ctx.rect);
    let lines = if index.state().read().is_processing() {
        let genesis_hash: H256 = genesis_info.header().hash().unpack();
        let capacity_list_result = with_index_db(index_dir, genesis_hash, |backend, cf| {
            let db =
                IndexDatabase::from_db(backend, cf, network_type, genesis_info.clone(), false)?;
            Ok(db.get_top_n(50))
        });
        match capacity_list_result {
            Ok(capacity_list) => capacity_list
                .iter()
                .flat_map(|(lock_hash, payload_opt, capacity)| {
                    vec![
                        Text::styled(
                            format!("{:x}", lock_hash),
                            Style::default().modifier(Modifier::BOLD),
                        ),
                        Text::raw(format!(
                            "  [address ]: {}",
                            payload_opt
                                .as_ref()
                                .map(|payload| Address::new(network_type, payload.clone(), true)
                                    .to_string())
                                .unwrap_or_else(|| "null".to_owned())
                        )),
                        Text::raw(format!(
                            "  [capacity]: {}.{} ({})",
                            capacity / ONE_CKB,
                            capacity % ONE_CKB,
                            human_capacity(*capacity)
                        )),
                    ]
                })
                .collect::<Vec<_>>(),
            Err(err) => vec![Text::raw(format!("Open db error: {}", err))],
        }
    } else {
        vec![Text::raw(index.state().read().to_string())]
    };
    List::new(lines.into_iter()).render(ctx.frame, top_capacity_chunks[0]);
}
