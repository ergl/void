extern crate climate;
extern crate termion;
#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;

use std::collections::BTreeMap;
use std::io::{Write, Stdout, stdout, stdin};
use std::cell::RefCell;
use std::ops::{Div, Mul};
use std::rc::Rc;
use std::sync::RwLock;

use log::{LogRecord, LogLevel, LogLevelFilter, LogMetadata, SetLoggerError};
use termion::event::{Key, Event, MouseEvent};
use termion::input::{TermRead, MouseTerminal};
use termion::raw::{IntoRawMode, RawTerminal};

struct ScreenLogger;

impl log::Log for ScreenLogger {
    fn enabled(&self, metadata: &LogMetadata) -> bool {
        metadata.level() <= LogLevel::Info
    }

    fn log(&self, record: &LogRecord) {
        if self.enabled(record.metadata()) {
            let line = format!("{} - {}", record.level(), record.args());
            let mut logs = LOGS.write().unwrap();
            logs.insert(0, line);
            logs.truncate(10);
        }
    }
}

pub fn init_screen_log() -> Result<(), SetLoggerError> {
    log::set_logger(|max_log_level| {
        max_log_level.set(LogLevelFilter::Debug);
        Box::new(ScreenLogger)
    })
}

lazy_static! {
    static ref LOGS: RwLock<Vec<String>> = RwLock::new(vec![]);
}

enum ScreenMode {
    Select,
    Edit,
}

struct Screen {
    anchors: BTreeMap<(u16, u16), Rc<RefCell<Anchor>>>,
    last_selected: Option<(Rc<RefCell<Anchor>>, Rc<RefCell<Node>>)>,
    stdout: Option<MouseTerminal<RawTerminal<Stdout>>>,
}

impl Default for Screen {
    fn default() -> Screen {
        Screen {
            anchors: BTreeMap::new(),
            last_selected: None,
            stdout: None,
        }
    }
}

impl Screen {
    fn draw(&mut self) {
        // clear screen
        print!("\x1b[2J\x1b[H");

        for (coords, anchor) in self.anchors.iter() {
            anchor.borrow().draw(coords.0, coords.1);
        }

        let (_, bottom) = termion::terminal_size().unwrap();
        println!("{}logs:", termion::cursor::Goto(0, bottom - 11));
        {
            let logs = LOGS.read().unwrap();
            for msg in logs.iter().rev() {
                println!("\r{}", msg);
            }
        }
        let mut s = self.stdout.take().unwrap();
        s.flush().unwrap();
        self.stdout = Some(s);
    }

    fn insert(&mut self, coords: (u16, u16), anchor: Anchor) {
        self.anchors.insert(coords, Rc::new(RefCell::new(anchor)));
    }

    fn lookup(&mut self, coords: (u16, u16)) -> Option<(Rc<RefCell<Anchor>>, Rc<RefCell<Node>>)> {
        // scan possible anchors
        let mut candidate_anchors = vec![];
        for (&(x, y), anchor) in self.anchors.iter() {
            if coords.0 >= x && coords.1 >= y && coords.1 - y < anchor.borrow().height() as u16 {
                candidate_anchors.push(((x, y), anchor.clone()));
            }
        }
        // scan possible nodes
        let mut candidate_nodes = vec![];
        for ((x, y), anchor) in candidate_anchors {
            if let Some(node) = anchor.borrow().lookup((coords.0 - x, coords.1 - y)) {
                candidate_nodes.push((anchor.clone(), node));
            }
        }
        candidate_nodes.pop()
    }

    fn try_select(&mut self, x: u16, y: u16) {
        if let Some((_, ref old_node)) = self.last_selected {
            old_node.borrow_mut().selected = false;
        }
        if let Some((anchor, node)) = self.lookup((x, y)) {
            node.borrow_mut().selected = true;
            self.last_selected = Some((anchor, node.clone()))
        }
    }

    fn delete_selected(&mut self) {
        if let Some((ref anchor, ref node)) = self.last_selected {
            let ptr = {
                anchor.borrow().head.as_ptr()
            };
            if ptr == node.as_ptr() {
                info!("deleting anchor {:?}", node.borrow().content);
                // nuke whole anchor
                let anchors = self.anchors
                    .clone()
                    .into_iter()
                    .filter(|&(ref coords, ref anchor)| anchor.borrow().head.as_ptr() != ptr)
                    .collect();
                self.anchors = anchors;
            } else {
                let anchor = anchor.borrow();
                anchor.head.borrow_mut().delete(node.clone());
            }
        }
    }

    fn create_child(&mut self) {
        if let Some((ref anchor, ref selected)) = self.last_selected {
            selected.borrow_mut().create_child()
        }
    }

    fn run(&mut self) {
        if self.stdout.is_none() {
            self.stdout = Some(MouseTerminal::from(stdout().into_raw_mode().unwrap()));
        }
        self.draw();
        let stdin = stdin();
        for c in stdin.events() {
            let evt = c.unwrap();
            self.handle_event(evt);
            self.draw();
        }
    }

    fn toggle_collapsed(&mut self) {
        if let Some((ref anchor, ref selected)) = self.last_selected {
            selected.borrow_mut().toggle_collapsed()
        }
    }

    fn create_anchor(&mut self, coords: (u16, u16)) {
        let header = node("new", vec![]);
        let anchor = Anchor { head: Rc::new(RefCell::new(header)) };
        self.insert(coords, anchor);
    }

    fn backspace(&mut self) {
        if let Some((ref anchor, ref selected)) = self.last_selected {
            let mut node = selected.borrow_mut();
            node.content.backspace();
        }
    }

    fn append(&mut self, c: char) {
        if let Some((ref anchor, ref selected)) = self.last_selected {
            let mut node = selected.borrow_mut();
            node.content.append(c);
        }
    }

    fn handle_event(&mut self, evt: Event) {
        match evt {
            Event::Key(Key::Char('\n')) => self.toggle_collapsed(),
            Event::Key(Key::Char('\t')) => self.create_child(),
            Event::Key(Key::Delete) => self.delete_selected(),
            Event::Key(Key::Alt('\u{1b}')) => self.exit(),
            Event::Key(Key::Backspace) => self.backspace(),
            Event::Key(Key::Char(c)) => self.append(c),
            Event::Mouse(me) => {
                match me {
                    MouseEvent::Press(_, x, y) => {
                        self.try_select(x, y);
                        if self.last_selected.is_none() {
                            self.create_anchor((x, y));
                        }
                    }
                    MouseEvent::Release(x, y) => {}
                    e => warn!("Weird mouse event {:?}", e),
                }
            }
            e => warn!("Weird event {:?}", e),
        }
    }

    fn exit(&self) {
        let (_, bottom) = termion::terminal_size().unwrap();
        print!("{}", termion::cursor::Goto(0, bottom));
        std::process::exit(0);
    }
}

struct Anchor {
    head: Rc<RefCell<Node>>,
}

impl Anchor {
    fn draw(&self, x: u16, y: u16) {
        self.head.borrow().draw("".to_string(), x, y, false);
    }
    fn lookup(&self, coords: (u16, u16)) -> Option<Rc<RefCell<Node>>> {
        let head = self.head.borrow();
        if coords.1 == 0 {
            if head.content.len() + 1 >= coords.0 as usize {
                Some(self.head.clone())
            } else {
                None
            }
        } else {
            head.lookup(0, coords)
        }
    }

    fn height(&self) -> usize {
        self.head.borrow().height()
    }
}

#[derive(Debug)]
enum Content {
    Text {
        text: String,
    },
    Plot(Vec<i64>),
}

impl Content {
    fn draw(&self) {
        match self {
            &Content::Text { text: ref text } => print!("{}", text),
            &Content::Plot(ref data) => plot_graph(data.clone()),
        }
    }
    fn len(&self) -> usize {
        match self {
            &Content::Text { text: ref text } => text.len(),
            &Content::Plot(ref data) => data.len(),
        }
    }
    fn backspace(&mut self) {
        match self {
            &mut Content::Text { text: ref mut text } => {
                let newlen = std::cmp::max(text.len(), 1) - 1;
                *text = text.clone()[..newlen].to_string();
            }
            &mut Content::Plot(ref data) => unimplemented!(),
        }
    }
    fn append(&mut self, c: char) {
        match self {
            &mut Content::Text { text: ref mut text } => {
                text.push(c);
            }
            &mut Content::Plot(ref data) => {
                unimplemented!();
            }
        }
    }
}

#[derive(Debug)]
struct Node {
    content: Content,
    children: Vec<Rc<RefCell<Node>>>,
    selected: bool,
    collapsed: bool,
}

impl Node {
    fn draw(&self, prefix: String, x: u16, y: u16, last: bool) -> usize {
        print!("{}", termion::cursor::Goto(x, y));

        if self.selected {
            print!("{}", termion::style::Invert);
        }

        if prefix == "" {
            print!("⚒ ");
        }

        print!("{}", prefix);

        if prefix != "" {
            if last {
                print!("└─ ");
            } else {
                print!("├─ ");
            }
        }

        self.content.draw();

        if self.collapsed {
            print!("…");
        }

        if self.selected {
            print!("{}", termion::style::Reset);
        }

        println!("");

        let mut drawn = 1;
        let mut prefix = prefix;
        if last {
            prefix.push_str("   ");
        } else if prefix == "" {
            prefix.push_str("  ");
        } else {
            prefix.push_str("│  ");
        }
        if !self.collapsed {
            let n_children = self.children.len();
            for (n, child) in self.children.iter().enumerate() {
                let last = if n + 1 == n_children {
                    true
                } else {
                    false
                };
                drawn += child.borrow().draw(prefix.clone(), x, y + drawn as u16, last);
            }
        }

        drawn
    }
    fn lookup(&self, depth: usize, coords: (u16, u16)) -> Option<Rc<RefCell<Node>>> {
        let mut y_traversed = 1;
        for child in self.children.iter() {
            if coords.1 == y_traversed {
                if child.borrow().content.len() + 1 + (3 * (depth + 1)) >= coords.0 as usize {
                    return Some(child.clone());
                } else {
                    return None;
                }
            } else if coords.1 < y_traversed + child.borrow().height() as u16 {
                return child.borrow().lookup(depth + 1, (coords.0, coords.1 - y_traversed));
            } else {
                y_traversed += child.borrow().height() as u16;
            }
        }

        None
    }

    fn height(&self) -> usize {
        if self.collapsed {
            1
        } else {
            self.children.iter().fold(1, |acc, c| acc + c.borrow().height())
        }
    }

    fn delete(&mut self, node: Rc<RefCell<Node>>) -> bool {
        let ptr = {
            node.as_ptr()
        };
        let mut contains = false;
        for child in self.children.iter() {
            if ptr == child.as_ptr() {
                info!("deleting child {:?}", node.borrow().content);
                contains = true;
            }
        }
        if contains {
            let children = self.children.clone();
            let new_children = children.into_iter().filter(|c| ptr != c.as_ptr()).collect();
            self.children = new_children;
            return true;
        }
        self.children.iter().fold(false, |acc, c| {
            if acc {
                true
            } else {
                c.borrow_mut().delete(node.clone())
            }
        })
    }
    fn toggle_collapsed(&mut self) {
        if self.collapsed {
            self.collapsed = false;
        } else {
            self.collapsed = true;
        }
    }
    fn create_child(&mut self) {
        let new = node("new", vec![]);
        self.children.push(Rc::new(RefCell::new(new)));
    }
}

fn node(text: &str, children: Vec<Node>) -> Node {
    let rc_children = children.into_iter().map(|child| Rc::new(RefCell::new(child))).collect();

    Node {
        content: Content::Text { text: text.to_string() },
        children: rc_children,
        selected: false,
        collapsed: false,
    }
}

fn plot_graph<T>(nums_in: Vec<T>)
    where T: Into<i64>
{
    const bars: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    let nums: Vec<_> = nums_in.into_iter().map(|n| n.into()).collect();
    let max = nums.iter().max();

    for n in nums.iter() {
        let idx = (bars.len() - 1) as i64 * n / max.unwrap();
        print!("{}", bars[idx as usize]);
    }
}

fn main() {
    init_screen_log();
    let other = node("other", vec![]);
    let next = node("next", vec![]);
    let zone = node("zone", vec![]);
    let plot = Node {
        content: Content::Plot(vec![1, 2, 5, 2, 3]),
        children: vec![],
        selected: false,
        collapsed: false,
    };
    let bone = node("bone", vec![plot]);
    let one = node("one", vec![bone, zone]);
    let header = node("header", vec![one, next, other]);

    let mut anchor = Anchor { head: Rc::new(RefCell::new(header)) };

    let mut scene = Screen::default();
    scene.insert((3, 4), anchor);
    scene.run();
}