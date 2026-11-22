//! The play queue: what plays next, with shuffle and repeat. It knows
//! nothing about audio, so both the GStreamer engine and the silent one
//! use it.

use crate::model::Track;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

impl Repeat {
    pub fn cycle(self) -> Repeat {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }
}

#[derive(Debug, Default)]
pub struct Queue {
    items: Vec<Track>,
    /// Indexes into `items`, in play order.
    order: Vec<usize>,
    pos: Option<usize>,
    shuffle: bool,
    pub repeat: Repeat,
    seed: u64,
}

impl Queue {
    pub fn new() -> Queue {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9e3779b97f4a7c15)
            | 1;
        Queue { seed, ..Queue::default() }
    }

    /// A queue with a fixed shuffle seed, for tests.
    pub fn with_seed(seed: u64) -> Queue {
        Queue { seed: seed | 1, ..Queue::default() }
    }

    fn rand(&mut self) -> u64 {
        // xorshift64
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.seed = x;
        x
    }

    /// Fisher-Yates over `order[from..]`.
    fn shuffle_tail(&mut self, from: usize) {
        let len = self.order.len();
        for i in (from + 1..len).rev() {
            let j = from + (self.rand() % (i - from + 1) as u64) as usize;
            self.order.swap(i, j);
        }
    }

    /// Replaces the queue and starts at `start` (an index into `items`).
    pub fn set(&mut self, items: Vec<Track>, start: usize) {
        self.items = items;
        self.order = (0..self.items.len()).collect();
        if self.items.is_empty() {
            self.pos = None;
            return;
        }
        let start = start.min(self.items.len() - 1);
        if self.shuffle {
            self.order.swap(0, start);
            self.shuffle_tail(1);
            self.pos = Some(0);
        } else {
            self.pos = Some(start);
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
        self.pos = None;
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    /// Turns shuffle on or off without interrupting the current song: it
    /// stays current, and the order around it changes.
    pub fn set_shuffle(&mut self, on: bool) {
        if on == self.shuffle {
            return;
        }
        self.shuffle = on;
        let Some(pos) = self.pos else { return };
        let current = self.order[pos];
        if on {
            self.order = (0..self.items.len()).collect();
            self.order.swap(0, current);
            self.shuffle_tail(1);
            self.pos = Some(0);
        } else {
            self.order = (0..self.items.len()).collect();
            self.pos = Some(current);
        }
    }

    pub fn current(&self) -> Option<&Track> {
        self.pos.map(|p| &self.items[self.order[p]])
    }

    /// Moves to the next song. `auto` is true when the last one ended on its
    /// own, which is when Repeat One repeats; pressing Next moves on.
    /// Returns the new current song, or None when the queue is over.
    pub fn next(&mut self, auto: bool) -> Option<&Track> {
        let pos = self.pos?;
        if auto && self.repeat == Repeat::One {
            return self.current();
        }
        if pos + 1 < self.order.len() {
            self.pos = Some(pos + 1);
        } else if auto && self.repeat == Repeat::Off {
            return None;
        } else {
            // Past the end: start over (reshuffled when shuffling).
            if self.shuffle {
                self.shuffle_tail(0);
            }
            self.pos = Some(0);
        }
        self.current()
    }

    /// Moves to the previous song, or stays on the first.
    pub fn previous(&mut self) -> Option<&Track> {
        let pos = self.pos?;
        if pos > 0 {
            self.pos = Some(pos - 1);
        } else if self.repeat == Repeat::All {
            self.pos = Some(self.order.len() - 1);
        }
        self.current()
    }

    /// The songs after the current one, in play order.
    pub fn upcoming(&self) -> Vec<&Track> {
        match self.pos {
            Some(p) => self.order[p + 1..].iter().map(|&i| &self.items[i]).collect(),
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn songs(n: usize) -> Vec<Track> {
        (0..n)
            .map(|i| Track { id: i as i64, title: format!("t{i}"), ..Track::default() })
            .collect()
    }

    fn ids(q: &mut Queue, auto: bool, steps: usize) -> Vec<i64> {
        let mut out = vec![q.current().unwrap().id];
        for _ in 0..steps {
            match q.next(auto) {
                Some(t) => out.push(t.id),
                None => break,
            }
        }
        out
    }

    #[test]
    fn plays_in_order_and_stops_at_the_end() {
        let mut q = Queue::with_seed(1);
        q.set(songs(3), 1);
        assert_eq!(ids(&mut q, true, 5), [1, 2]);
        assert!(q.next(true).is_none());
    }

    #[test]
    fn next_button_wraps_even_without_repeat() {
        let mut q = Queue::with_seed(1);
        q.set(songs(3), 2);
        assert_eq!(q.next(false).unwrap().id, 0);
    }

    #[test]
    fn repeat_all_wraps_and_repeat_one_holds() {
        let mut q = Queue::with_seed(1);
        q.set(songs(2), 0);
        q.repeat = Repeat::All;
        assert_eq!(ids(&mut q, true, 3), [0, 1, 0, 1]);
        q.repeat = Repeat::One;
        assert_eq!(q.next(true).unwrap().id, 1);
        assert_eq!(q.next(true).unwrap().id, 1);
        assert_eq!(q.next(false).unwrap().id, 0, "the Next button leaves a repeated song");
    }

    #[test]
    fn previous_stops_at_the_first_song() {
        let mut q = Queue::with_seed(1);
        q.set(songs(3), 1);
        assert_eq!(q.previous().unwrap().id, 0);
        assert_eq!(q.previous().unwrap().id, 0);
        q.repeat = Repeat::All;
        assert_eq!(q.previous().unwrap().id, 2);
    }

    #[test]
    fn shuffle_starts_with_the_chosen_song_and_plays_each_once() {
        let mut q = Queue::with_seed(42);
        q.set_shuffle(true);
        q.set(songs(8), 5);
        let played = ids(&mut q, true, 20);
        assert_eq!(played[0], 5);
        assert_eq!(played.len(), 8);
        let mut sorted = played.clone();
        sorted.sort();
        assert_eq!(sorted, (0..8).collect::<Vec<_>>());
        assert_ne!(played, [5, 6, 7, 0, 1, 2, 3, 4], "shuffled");
    }

    #[test]
    fn toggling_shuffle_keeps_the_current_song() {
        let mut q = Queue::with_seed(7);
        q.set(songs(6), 3);
        q.set_shuffle(true);
        assert_eq!(q.current().unwrap().id, 3);
        assert_eq!(q.upcoming().len(), 5);
        q.set_shuffle(false);
        assert_eq!(q.current().unwrap().id, 3);
        assert_eq!(q.next(true).unwrap().id, 4);
    }

    #[test]
    fn empty_queue_is_inert() {
        let mut q = Queue::new();
        assert!(q.current().is_none());
        assert!(q.next(true).is_none());
        assert!(q.previous().is_none());
        q.set(Vec::new(), 0);
        assert!(q.is_empty());
    }

    #[test]
    fn repeat_cycles() {
        assert_eq!(Repeat::Off.cycle(), Repeat::All);
        assert_eq!(Repeat::All.cycle(), Repeat::One);
        assert_eq!(Repeat::One.cycle(), Repeat::Off);
    }
}
