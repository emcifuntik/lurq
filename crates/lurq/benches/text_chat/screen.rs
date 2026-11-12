//! The chat screen the text benchmarks and the capture check draw: a channel
//! list, a chat panel of short messages in a `VirtualizedList` with a text
//! input under it, and a member list. Two font sizes and two weights, as a
//! chat client uses them, and an emoji in some messages.
//!
//! Everything is generated from the message's index, so two builds lay out
//! and paint the same screen.
use std::sync::Arc;

use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, ScrollVertical, Text, TextInput, VirtualizedList},
  core::Signal,
  layout::{
    Alignment,
    layout_kind::ScrollState,
    text_style::{FontWeight, TextStyle},
  },
  node::{Element, color::Color, dimension::Dimension},
};

/// The window's size in logical pixels: tall enough for about 40 messages.
pub const VIEWPORT: (u32, u32) = (1280, 2160);
/// Width of the channel list and of the member list.
pub const SIDE_WIDTH: f32 = 220.0;
pub const DRAFT_ID: &str = "draft";
pub const CHANNELS: usize = 40;
pub const MEMBERS: usize = 50;

const BODY_SIZE: f32 = 14.0;
const SMALL_SIZE: f32 = 11.0;
const LINE_HEIGHT: f32 = 1.4;

/// The words messages are made of, separated by spaces.
const WORDS: &str = "the of and to in is that for it as was with be by on not he this are or his from at which but \
                     have an had they you were their one all we can her has there been if more when will would who \
                     so no voice channel server latency packet codec stream screen share mute deafen push talk \
                     noise gate build release branch merge review deploy tonight tomorrow meeting later sounds \
                     good thanks maybe works broken fixed testing again please check update version driver window \
                     render frame glyph atlas shader buffer thread queue lock actually probably definitely anyone \
                     someone everything nothing something already still";

const EMOJI: &[&str] = &["😀", "👍", "🎉", "🔥", "😂", "🙏", "👀", "🚀"];

const AUTHORS: &[(&str, &str)] = &[
  ("Ada Lovelace", "#c2410c"),
  ("Grace Hopper", "#0369a1"),
  ("Alan Turing", "#15803d"),
  ("Edsger Dijkstra", "#7e22ce"),
  ("Barbara Liskov", "#be123c"),
  ("Donald Knuth", "#0f766e"),
  ("Margaret Hamilton", "#a16207"),
  ("Dennis Ritchie", "#1d4ed8"),
  ("Ken Thompson", "#4d7c0f"),
  ("Radia Perlman", "#9d174d"),
  ("Linus Torvalds", "#b45309"),
  ("Frances Allen", "#4338ca"),
];

const CHANNEL_NAMES: &[&str] = &[
  "general",
  "random",
  "announcements",
  "voice-chat",
  "screenshots",
  "dev",
  "dev-audio",
  "dev-video",
  "dev-net",
  "builds",
  "releases",
  "support",
  "feedback",
  "music",
  "games",
  "memes",
  "off-topic",
  "linux",
  "windows",
  "macos",
];

/// One chat message. Cheap to clone: the list hands every row its own copy.
#[derive(Clone, PartialEq)]
pub struct Message {
  pub id: u64,
  pub author: usize,
  pub time: Arc<str>,
  pub body: Arc<str>,
}

#[cfg(feature = "devtools")]
impl lurq::app::component::DevtoolsInspectable for Message {}

/// A small deterministic generator (xorshift64*), seeded per message.
struct Rng(u64);

impl Rng {
  fn new(seed: u64) -> Self {
    Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
  }

  fn next(&mut self) -> u64 {
    self.0 ^= self.0 >> 12;
    self.0 ^= self.0 << 25;
    self.0 ^= self.0 >> 27;
    self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
  }

  fn below(&mut self, bound: usize) -> usize {
    (self.next() >> 33) as usize % bound
  }
}

/// Message `index` of `channel`: one to three lines at the chat panel's
/// width, every sixth or so with an emoji.
pub fn message(channel: u64, index: u64) -> Message {
  let mut rng = Rng::new(channel.wrapping_mul(1_000_003).wrapping_add(index).wrapping_add(1));
  // About 40% one line, 35% two lines, 25% three lines at the panel's width.
  let words = match rng.below(20) {
    0..8 => 3 + rng.below(14),
    8..15 => 22 + rng.below(12),
    _ => 40 + rng.below(12),
  };
  let vocabulary: Vec<&str> = WORDS.split(' ').collect();
  let mut body = String::new();
  for word in 0..words {
    if word > 0 {
      body.push(' ');
    }
    body.push_str(vocabulary[rng.below(vocabulary.len())]);
  }
  if rng.below(6) == 0 {
    body.push(' ');
    body.push_str(EMOJI[rng.below(EMOJI.len())]);
  }
  let minutes = (index * 3 + channel * 7) % (24 * 60);
  Message {
    id: channel * 1_000_000 + index,
    author: rng.below(AUTHORS.len()),
    time: format!("Today at {:02}:{:02}", minutes / 60, minutes % 60).into(),
    body: body.into(),
  }
}

/// The first `count` messages of `channel`.
pub fn messages(channel: u64, count: usize) -> Arc<Vec<Message>> {
  Arc::new((0..count as u64).map(|index| message(channel, index)).collect())
}

fn style(size: f32, weight: FontWeight, color: &str) -> TextStyle {
  TextStyle {
    font_size: size,
    line_height: LINE_HEIGHT,
    weight,
    color: Color::from_hex(color),
    ..TextStyle::default()
  }
}

/// A style of each font face the screen draws with: regular and bold.
pub fn font_styles() -> [TextStyle; 2] {
  [
    style(BODY_SIZE, FontWeight::Normal, "#0f172a"),
    style(BODY_SIZE, FontWeight::Bold, "#0f172a"),
  ]
}

pub struct MessageRow;

impl Component for MessageRow {
  type Props = Message;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let message = ctx.props::<Message>();
    let (author, color) = AUTHORS[message.author];
    let fill = Dimension::Pct(100.0);
    Column::new()
      .width(fill)
      .padding_horizontal(12.0)
      .padding_vertical(3.0)
      .hovered(|style| style.background("#f1f5f9"))
      .child(
        Row::new()
          .spacing(8.0)
          .align_items(Alignment::Center)
          .child(Text::styled(author, style(BODY_SIZE, FontWeight::Bold, color)).nowrap())
          .child(Text::styled(&message.time, style(SMALL_SIZE, FontWeight::Normal, "#64748b")).nowrap()),
      )
      .child(Text::styled(&message.body, style(BODY_SIZE, FontWeight::Normal, "#0f172a")).width(fill))
  }
}

/// What the screen shows: a channel's messages, the draft in its input and
/// where the list is scrolled.
#[derive(Clone)]
pub struct ChatProps {
  pub channel: u64,
  pub messages: Arc<Vec<Message>>,
  pub draft: Signal<String>,
  pub scroll: ScrollState,
}

impl PartialEq for ChatProps {
  fn eq(&self, other: &Self) -> bool {
    self.channel == other.channel && Arc::ptr_eq(&self.messages, &other.messages)
  }
}

#[cfg(feature = "devtools")]
impl lurq::app::component::DevtoolsInspectable for ChatProps {}

impl ChatProps {
  pub fn new(channel: u64, count: usize) -> Self {
    Self {
      channel,
      messages: messages(channel, count),
      draft: Signal::new(String::new()),
      scroll: ScrollState::new(),
    }
  }

  /// The same screen showing another channel: every message is replaced.
  pub fn switched_to(&self, channel: u64, count: usize) -> Self {
    Self {
      channel,
      messages: messages(channel, count),
      draft: self.draft.clone(),
      scroll: self.scroll.clone(),
    }
  }
}

fn channel_name(index: usize) -> String {
  let name = CHANNEL_NAMES[index % CHANNEL_NAMES.len()];
  match index / CHANNEL_NAMES.len() {
    0 => format!("# {name}"),
    round => format!("# {name}-{round}"),
  }
}

fn section(title: &str) -> Text {
  Text::styled(title, style(SMALL_SIZE, FontWeight::Bold, "#475569")).nowrap()
}

fn channel_list(current: u64) -> Element {
  let mut list = Column::new()
    .width(Dimension::Pct(100.0))
    .padding(8.0)
    .spacing(2.0)
    .child(section("TEXT CHANNELS"));
  for index in 0..CHANNELS {
    // Every fifth channel has unread messages and the open one is marked.
    let weight = if index % 5 == 2 || index as u64 == current % CHANNELS as u64 {
      FontWeight::Bold
    } else {
      FontWeight::Normal
    };
    list = list.child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .padding_horizontal(8.0)
        .padding_vertical(2.0)
        .hovered(|style| style.background("#e2e8f0"))
        .child(Text::styled(&channel_name(index), style(BODY_SIZE, weight, "#1e293b")).nowrap()),
    );
  }
  ScrollVertical::new(list)
    .width(SIDE_WIDTH)
    .height(Dimension::Pct(100.0))
    .background("#f8fafc")
    .into()
}

fn member_list() -> Element {
  let mut list = Column::new().width(Dimension::Pct(100.0)).padding(8.0).spacing(2.0);
  for index in 0..MEMBERS {
    if index % 25 == 0 {
      list = list.child(section(if index == 0 { "ONLINE - 25" } else { "OFFLINE - 25" }));
    }
    let (name, color) = AUTHORS[index % AUTHORS.len()];
    let label = match index / AUTHORS.len() {
      0 => name.to_owned(),
      round => format!("{name} {}", round + 1),
    };
    list = list.child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .padding_horizontal(8.0)
        .padding_vertical(2.0)
        .hovered(|style| style.background("#e2e8f0"))
        .child(Text::styled(&label, style(BODY_SIZE, FontWeight::Normal, color)).nowrap()),
    );
  }
  ScrollVertical::new(list)
    .width(SIDE_WIDTH)
    .height(Dimension::Pct(100.0))
    .background("#f8fafc")
    .into()
}

pub struct ChatScreen;

impl Component for ChatScreen {
  type Props = ChatProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<ChatProps>().clone();
    let fill = Dimension::Pct(100.0);
    let name = channel_name((props.channel % CHANNELS as u64) as usize);
    let header = Column::new()
      .width(fill)
      .padding_horizontal(12.0)
      .padding_vertical(8.0)
      .child(Text::styled(&name, style(BODY_SIZE, FontWeight::Bold, "#0f172a")).nowrap())
      .child(
        Text::styled(
          "Voice and text for the team: releases, builds and whatever broke today",
          style(SMALL_SIZE, FontWeight::Normal, "#64748b"),
        )
        .nowrap(),
      );
    let list = VirtualizedList::new_shared(ctx, props.messages.clone())
      .with_scroll_state(props.scroll.clone())
      .width(fill)
      .flex(1.0)
      .mount_keyed::<MessageRow, _, _, _>(|message| message.id, |message| message.clone());
    let input = TextInput::styled(props.draft.clone(), style(BODY_SIZE, FontWeight::Normal, "#0f172a"))
      .id(DRAFT_ID)
      .placeholder(&format!("Message {name}"))
      .width(fill)
      .padding(10.0);
    let chat = Column::new()
      .flex(1.0)
      .height(fill)
      .background("#ffffff")
      .child(header)
      .child(list)
      .child(input);
    Row::new()
      .width(fill)
      .height(fill)
      .child(channel_list(props.channel))
      .child(chat)
      .child(member_list())
  }
}
