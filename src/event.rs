use crossterm::event::{Event, EventStream, KeyEvent, KeyEventKind};
use futures_util::StreamExt;
use tokio::sync::mpsc::UnboundedSender;

use crate::feeds::FeedMsg;

/// Everything the app loop reacts to arrives as a `Msg` on one channel.
pub enum Msg {
    Key(KeyEvent),
    Resize,
    Feed(FeedMsg),
}

pub fn spawn_input(tx: UnboundedSender<Msg>) {
    tokio::spawn(async move {
        let mut stream = EventStream::new();
        while let Some(Ok(event)) = stream.next().await {
            let msg = match event {
                Event::Key(key) if key.kind == KeyEventKind::Press => Msg::Key(key),
                Event::Resize(..) => Msg::Resize,
                _ => continue,
            };
            if tx.send(msg).is_err() {
                break;
            }
        }
    });
}
