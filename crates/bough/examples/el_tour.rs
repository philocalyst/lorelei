//! A tour of the whole author surface (`bough::prelude`), written as one small music-library app.
//!
//! Every builder, port, child form, route shape, async form and motion form is used at least once, in the
//! combinations an app actually needs: rows that are views of their own, routes that reach the row they were
//! written in (or the root above it), a producer feeding a reader that a boundary guards, motion driven by the
//! same signals the handlers write, and `changed` as the one effect site.
//!
//!     cargo run --release -p bough --example el_tour            # a window
//!     cargo run --release -p bough --example el_tour -- --check # headless: drive it, check every frame
//!
//! `--check` drives a scripted session (clicks, keys, a drag, a search that completes, list edits, a
//! filter) through the test driver and asserts after every step that the retained frame equals a full redraw.
use bough::prelude::*;

// ------------------------------------------------------------------------------------------- theme

const INK: Color = rgb(0x1d2330);
const MUTED: Color = rgb(0x6b7385);
const SURFACE: Color = rgb(0xffffff);
const PANEL: Color = rgb(0xf3f5f9);
const ACCENT: Color = rgb(0x2f6bff);
const HOT: Color = rgb(0xffe9a8);
const GOOD: Color = rgb(0x1f9d55);
const BAD: Color = rgb(0xd64545);

/// A layout recipe as a constant (02 §2): `.layout(BAR)`.
const BAR: Flex = Flex::row().align(Align::Center).gap(8.0);
const STACK: Flex = Flex::column().gap(6.0);

/// A mixin: any element in a chain takes it with `.apply(card)`.
fn card(e: El<'_>) -> El<'_> {
    e.bg(SURFACE).rounded(8).border(1, rgb(0xdde2ea)).p(10)
}

/// A helper view returning one of several shapes: every arm is an `El`.
fn badge<'f>(label: &'f str, tone: Tone) -> El<'f> {
    let (bg, fg) = match tone {
        Tone::Good => (rgb(0xdcf5e6), GOOD),
        Tone::Bad => (rgb(0xfde2e2), BAD),
        Tone::Plain => (PANEL, MUTED),
    };
    row(text(label).fg(fg).font(11, 14)).px(6).py(2).rounded(4).bg(bg)
}
#[derive(Clone, Copy)]
enum Tone {
    Good,
    Bad,
    Plain,
}

// ------------------------------------------------------------------------------------------- a row (a view)

/// One track: a view of its own. Its routes run on *this* track (the list records the edit), its signals are
/// direct ports, and its hover and focus survive any reorder of the list (rows are keyed by slot).
struct Track {
    title: Signal<String>,
    artist: String,
    plays: Signal<u32>,
    stars: Signal<u8>,
    starred: Signal<f32>,
    width: Signal<f32>,
    /// The drag's travel already applied (a drag event carries the travel since the press).
    dragged: f32,
}

impl Track {
    fn new(title: &str, artist: &str, plays: u32) -> Track {
        Track {
            title: Signal::new(title.into()),
            artist: artist.into(),
            plays: Signal::new(plays),
            stars: Signal::new(0),
            starred: Signal::new(0.0),
            width: Signal::new(220.0),
            dragged: 0.0,
        }
    }
    /// `fn(&mut Self)`: a method is a route.
    fn play(&mut self) {
        *self.plays += 1;
    }
    /// A method taking a value, bound with `.with(n)`.
    fn rate(&mut self, n: u8) {
        self.stars.set(n);
        self.starred.set(if n >= 4 { 1.0 } else { 0.0 });
    }
    /// `fn(&mut Self, &Ev)`: the event comes along (a drag's travel since the press).
    fn resize(&mut self, e: &Ev) {
        let step = e.dx - self.dragged;
        self.dragged = if e.on { e.dx } else { 0.0 };
        let w = (*self.width + step * 0.25).clamp(160.0, 320.0);
        self.width.set(w);
    }
    fn clear_rating(&mut self) {
        self.rate(0);
    }
}

impl View for Track {
    fn view(&self) -> El<'_> {
        // a motion on a signal the routes write: the star glow springs in and out
        let glow = animate(&self.starred).spring(Spring::bouncy());
        row((
            column((
                text(&self.title).fg(INK).font(14, 18),
                // a borrowed string field of a plain field: a constant of the description
                text(self.artist.as_str()).fg(MUTED).font(11, 14),
            ))
            .grow(1.0),
            // an integer signal is a text port: publishing it reruns no view
            text(&self.plays).fg(MUTED).w(36),
            // five stars, each a bound route on the row; the iterator is a dynamic child part
            row((1..=5u8).map(|n| {
                div()
                    .size(12, 12)
                    .rounded(6)
                    .bg(self.stars.map(move |s| if *s >= n { ACCENT } else { rgb(0xd5dae3) }))
                    .hover(|h| h.bg(HOT))
                    .on_click(Track::rate.with(n))
            }))
            .gap(3),
            // a spring-driven glow: opacity and a lift from one motion (reverse/scale map the same value)
            div().size(8, 8).rounded(4).bg(HOT).opacity(glow).translate_y(glow.reverse().scale(-3)),
        ))
        .layout(BAR)
        .w(&self.width)
        .p(6)
        .rounded(6)
        .bg(PANEL)
        .hover(|h| h.bg(rgb(0xe8edf7)))
        .on_click(Track::play)
        .on_drag(Track::resize)
        .on_key(Key::Backspace, Track::clear_rating)
        .on_key(Key::Char('5'), Track::rate.with(5))
        // a route to the root from inside a row: resolved up the owner chain at mount
        .on_key(Key::Delete, Library::remove_selected)
        .focusable()
    }
}

/// An explicit row function asking for its slot (02 §4.1: `list.each(f)`), so a route can carry it. A `fn`
/// item, because the row borrows its item for the description (`for<'a> fn(&'a T, Slot<T>) -> El<'a>`).
fn track_line(t: &Track, s: Slot<Track>) -> El<'_> {
    row((text(&t.title).fg(INK).font(12, 16), text(t.artist.as_str()).fg(MUTED).font(11, 14)))
        .layout(BAR)
        .on_click(Library::select.with(s))
}

// ------------------------------------------------------------------------------------------- the root

struct Library {
    tracks: List<Track>,
    query: Signal<String>,
    results: Reader<String>,
    filter_hot: bool,
    show_help: bool,
    panel: Signal<f32>,
    accent: Signal<Color>,
    total: Signal<u32>,
    log: Vec<String>,
    selected: Option<Slot<Track>>,
}

/// The producer: a request from the query (a plain `fn`, so what it depends on is in its argument).
fn search(q: &String) -> Request<String> {
    let q = q.clone();
    Request::fallible(move || {
        if q.contains('!') {
            Err(format!("bad query {q:?}"))
        } else {
            Ok(if q.is_empty() {
                "type to search".into()
            } else {
                format!("3 matches for {q:?}")
            })
        }
    })
}

impl Library {
    fn new() -> Library {
        let tracks = List::from_iter_in([
            Track::new("Blue in Green", "Miles Davis", 12),
            Track::new("Teardrop", "Massive Attack", 40),
            Track::new("Hyperballad", "Björk", 7),
            Track::new("Windowlicker", "Aphex Twin", 3),
        ]);
        Library {
            tracks,
            query: Signal::new(String::new()),
            results: Reader::new(),
            filter_hot: false,
            show_help: true,
            panel: Signal::new(0.0),
            accent: Signal::new(ACCENT),
            total: Signal::new(0),
            log: Vec::new(),
            selected: None,
        }
    }
    fn add(&mut self) {
        let n = self.tracks.len() + 1;
        let s = self.tracks.push(Track::new(&format!("Untitled {n}"), "You", 0));
        self.selected = Some(s);
    }
    fn shuffle(&mut self) {
        self.tracks.reverse();
        if self.tracks.len() > 2 {
            self.tracks.move_to(0, 2);
        }
    }
    fn sort_by_plays(&mut self) {
        self.tracks.sort_by_key(|t| std::cmp::Reverse(*t.plays));
    }
    fn remove_selected(&mut self) {
        if let Some(s) = self.selected.take() {
            self.tracks.take(s);
        } else {
            self.tracks.pop();
        }
    }
    fn toggle_hot(&mut self) {
        self.filter_hot = !self.filter_hot;
    }
    fn toggle_panel(&mut self) {
        let open = *self.panel < 0.5;
        self.panel.set(if open { 1.0 } else { 0.0 });
        self.accent.set(if open { rgb(0x8a3ffc) } else { ACCENT });
    }
    fn dismiss_help(&mut self) {
        self.show_help = false;
    }
    fn select(&mut self, s: Slot<Track>) {
        self.selected = Some(s);
    }
    /// Any key no element nearer the focus took.
    fn typed(&mut self, e: &Ev) {
        if let Some(Key::Char(c)) = e.key {
            if e.mods.ctrl && c == 'k' {
                self.query.set(String::new());
            }
        }
    }
}

impl View for Library {
    fn view(&self) -> El<'_> {
        let open = animate(&self.panel).over(260).ease(Ease::InOutCubic);
        let tint = animate(&self.accent).over(400).delay(60);
        let hot = |t: &Track| *t.stars >= 4;
        column((
            // header: a fixed tuple of children, a mapped colour port driven by a motion, an integer port
            row((
                text("Library").font(20, 26).fg(INK),
                badge("beta", Tone::Plain),
                div().grow(1.0),
                text(self.total.map(|n| format!("{n} plays"))).fg(MUTED),
                button("Add").on_click(Library::add),
                button("Shuffle").on_click(Library::shuffle),
                button("Most played").on_click(Library::sort_by_plays),
                button(if self.filter_hot { "All" } else { "Hot only" }).on_click(Library::toggle_hot),
                button("Panel").on_click(Library::toggle_panel),
            ))
            .layout(BAR)
            .p(12)
            .bg(SURFACE)
            .border(1, rgb(0xe3e7ee)),
            // an optional child: `Option<El>` inserts and removes at its position
            self.show_help.then(|| {
                row((para("Click a row to play it, the dots to rate it, drag a row to resize it; Delete removes the selected track, Backspace clears a rating.").fg(MUTED).w(420), button("Got it").on_click(Library::dismiss_help)))
                    .layout(BAR)
                    .apply(card)
            }),
            row((
                // the list: every row is the item's own view, keyed by slot
                column(&self.tracks).layout(STACK).w(360),
                column((
                    // search: a two-way input, a producer feeding a reader, a boundary guarding it
                    text_input(&self.query).input_size(220, 28),
                    produce(&self.query, &self.results, search),
                    boundary()
                        .loading(text("searching…").fg(MUTED))
                        .failed(badge("search failed", Tone::Bad))
                        .child(text(self.results.map(|r| r.cloned().unwrap_or_default())).fg(INK)),
                    div().size(220, 3).bg(&self.accent).opacity(is_pending(&self.results).map(|p| if p { 1.0 } else { 0.0 })),
                    text(latest(&self.query).map(|q| format!("typing: {q}"))).fg(MUTED).font(11, 14),
                    // an explicit projection with the row's slot, and a filter over plain data
                    column(self.tracks.each(track_line)).layout(STACK),
                    column(self.tracks.iter().filter(|t| !self.filter_hot || hot(t)).map(|t| badge(t.artist.as_str(), if hot(t) { Tone::Good } else { Tone::Plain }))).layout(STACK),
                    // a Vec of elements and an array of elements
                    column(self.log.iter().rev().take(3).map(|l| text(l.as_str()).fg(MUTED).font(10, 13)).collect::<Vec<_>>()),
                    row([badge("a", Tone::Plain), badge("b", Tone::Plain), badge("c", Tone::Plain)]).gap(4),
                ))
                .layout(STACK)
                .apply(card),
                // a sliding side panel: width, offset and fill all follow motions
                column((text("Now playing").fg(SURFACE), text(self.tracks.get(0).map_or("—", |t| t.artist.as_str())).fg(SURFACE)))
                    .layout(STACK)
                    .p(10)
                    .w(open.scale(180))
                    .translate_x(open.reverse().scale(40))
                    .opacity(open)
                    .bg(tint)
                    .clip_overflow(),
            ))
            .gap(12)
            .p(12)
            .items(Align::Start),
        ))
        .bg(PANEL)
        .w_pct(1.0)
        .h_pct(1.0)
        .on_any_key(Library::typed)
    }

    /// The one effect site: what crossed this view's boundaries this turn.
    fn changed(&mut self, d: Changed<'_, Self>) {
        if let Some(ops) = d.list(&self.tracks) {
            let total: u32 = self.tracks.iter().map(|t| *t.plays).sum();
            self.total.set(total);
            self.log.push(format!("{} list op(s), {total} plays", ops.len()));
        }
        if d.reader(&self.results) {
            self.log.push(format!("search: {}", self.results.latest().cloned().unwrap_or_default()));
        }
    }
}

// ------------------------------------------------------------------------------------------- run / check

fn main() {
    if std::env::args().any(|a| a == "--check") {
        check();
        return;
    }
    Application::run(Window::document(Library::new()).title("lorelei · El tour").size(900.0, 560.0));
}

/// A scripted session on the test driver; every frame is checked against a full redraw, and every step's
/// effect on the state is asserted.
fn check() {
    use bough::raw::{App, Button, Config};
    let font = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fonts/dejavu/DejaVuSans.ttf")).expect("fonts/dejavu");
    let mut app = App::new(Library::new(), Config::new(900.0, 560.0, 1.0, font), true);
    let mut frames = 0;
    let mut shot = |app: &mut App<Library>, what: &str| {
        app.frame();
        assert_eq!(
            app.presented_pixels(),
            app.reference_pixels(),
            "{what}: the retained frame equals a full redraw"
        );
        frames += 1;
    };
    let click = |app: &mut App<Library>, x: f32, y: f32| {
        app.pointer_move(x, y);
        app.pointer_down(x, y, Button::Primary);
        app.pointer_up(x, y, Button::Primary);
    };
    let settle = |app: &mut App<Library>| {
        for _ in 0..90 {
            app.tick(1.0 / 60.0);
        }
    };
    let plays = |l: &Library| l.tracks.iter().map(|t| *t.plays).collect::<Vec<_>>();
    shot(&mut app, "mount");

    // the producer asked at mount: the boundary shows its loading arm until the job completes
    assert!(app.owner.results.is_pending());
    app.run_jobs(8);
    app.poll();
    shot(&mut app, "first search");
    assert_eq!(app.owner.results.latest().map(String::as_str), Some("type to search"));

    // row routes (the rows sit below the header and the help card; see the layout in `view`)
    click(&mut app, 40.0, 170.0);
    shot(&mut app, "play the first row");
    assert_eq!(plays(&app.owner), [13, 40, 7, 3], "a click on a row plays *that* row");
    assert_eq!(*app.owner.total, 63, "`changed` saw the list edit and summed the plays");
    click(&mut app, 207.0, 220.0);
    settle(&mut app);
    shot(&mut app, "rate the second row 5");
    assert_eq!(
        (*app.owner.tracks.get(1).unwrap().stars, *app.owner.tracks.get(1).unwrap().starred),
        (5, 1.0),
        "a bound route on a dot inside a row"
    );
    assert_eq!(plays(&app.owner)[1], 40, "a dot's click is the dot's, not the row's");

    // a drag on the third row: past the slop it is a drag (the row widens), not a play
    app.pointer_move(40.0, 270.0);
    app.pointer_down(40.0, 270.0, Button::Primary);
    app.pointer_move(120.0, 270.0);
    app.pointer_up(120.0, 270.0, Button::Primary);
    shot(&mut app, "drag the third row");
    assert_eq!(
        (*app.owner.tracks.get(2).unwrap().width, plays(&app.owner)[2]),
        (240.0, 7),
        "dragged 80 points: 220 + 80/4"
    );

    // keys on the focused row (the drag focused it): a bound key route on the row, Backspace its method
    app.key_down(Key::Char('5'), Mods::default());
    assert_eq!(*app.owner.tracks.get(2).unwrap().stars, 5);
    app.key_down(Key::Backspace, Mods::default());
    settle(&mut app);
    shot(&mut app, "keys on a row");
    assert_eq!(*app.owner.tracks.get(2).unwrap().stars, 0);

    // search: a query the producer turns into a request, a completion, a failure
    app.turn(|l| {
        l.query.set("blue".into());
    });
    shot(&mut app, "query");
    assert!(app.owner.results.is_pending());
    app.run_jobs(8);
    app.poll();
    shot(&mut app, "search completes");
    assert_eq!(app.owner.results.latest().map(String::as_str), Some("3 matches for \"blue\""));
    app.turn(|l| {
        l.query.set("oops!".into());
    });
    app.run_jobs(8);
    app.poll();
    shot(&mut app, "search fails");
    assert!(app.owner.results.fault().is_some());
    app.key_down(
        Key::Char('k'),
        Mods {
            ctrl: true,
            ..Mods::default()
        },
    );
    shot(&mut app, "ctrl-k");
    assert_eq!(*app.owner.query, "", "an any-key route on the root clears the query");

    // root routes from the header, each followed by its motions settling
    let n = app.owner.tracks.len();
    for (f, what) in [
        (Library::add as fn(&mut Library), "add"),
        (Library::shuffle, "shuffle"),
        (Library::sort_by_plays, "sort"),
        (Library::toggle_hot, "filter"),
        (Library::toggle_panel, "panel"),
        (Library::dismiss_help, "help"),
    ] {
        app.turn(f);
        shot(&mut app, what);
        settle(&mut app);
        shot(&mut app, "motion settled");
    }
    assert_eq!(app.owner.tracks.len(), n + 1);
    assert_eq!(plays(&app.owner)[0], 40, "sorted by plays");
    assert_eq!(*app.owner.panel, 1.0);

    // Delete on a focused row runs the *root's* route from inside the row (the owner chain)
    click(&mut app, 40.0, 100.0);
    app.key_down(Key::Delete, Mods::default());
    shot(&mut app, "delete from a row");
    assert_eq!(app.owner.tracks.len(), n, "the root's route ran from the row");

    let l = &app.owner;
    println!("el_tour: {frames} frames, every retained frame equal to a full redraw");
    println!(
        "  {} tracks, plays {:?}, total {}, log tail {:?}",
        l.tracks.len(),
        plays(l),
        *l.total,
        l.log.iter().rev().take(3).collect::<Vec<_>>()
    );
}
