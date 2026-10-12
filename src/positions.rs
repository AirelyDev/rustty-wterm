//! 64-slot tracked-position table and the prompt finder. Ports of
//! `PositionTracker` (wasm_api.zig 156-169) and `find_prompt` /
//! `track_position` / `resolve_position` / `release_position` (1327-1406).
//!
//! Each entry keeps the screen it was tracked on together with that screen's
//! `ScreenSet` key and generation. `ScreenSet::remove` bumps the generation
//! and frees the screen (and every pin tracked in it), so an entry whose
//! `(key, screen, generation)` no longer matches the set must never be
//! dereferenced: its pin memory is gone. Every unsafe access below checks
//! `live` first.

use rustty_vt::page::SemanticPrompt;
use rustty_vt::pagelist::iter::Direction;
use rustty_vt::pagelist::Pin;
use rustty_vt::point::{Coordinate, Point, Tag};
use rustty_vt::screen::Screen;
use rustty_vt::screen_set::{Key, ScreenSet};
use rustty_vt::terminal::Terminal;

pub const POSITION_SLOTS: usize = 64;

#[derive(Clone, Copy)]
struct Entry {
    id: u32,
    pin: *mut Pin,
    screen: *mut Screen,
    key: Key,
    generation: usize,
}

impl Entry {
    /// The screen this entry was tracked on still exists in the set.
    fn live(&self, screens: &ScreenSet) -> bool {
        screens.get(self.key) == Some(self.screen)
            && screens.generation(self.key) == self.generation
    }

    /// Untrack the pin if its screen still exists. A removed screen freed
    /// its tracked pins with it.
    fn release(self, screens: &ScreenSet) {
        if self.live(screens) {
            // SAFETY: `live` proved `screen` is the set's current, allocated
            // screen for `key`; `pin` was returned by its `track_pin` and is
            // untracked at most once (the slot is cleared by the caller).
            // The screen is reached through the set's raw pointer, not
            // through the `&ScreenSet` borrow.
            unsafe { (*self.screen).pages.untrack_pin(self.pin) };
        }
    }
}

/// Fixed table bounding host-owned tracking resources. IDs never alias
/// released or invalidated handles, even when their slots are reused.
pub(crate) struct PositionTracker {
    entries: [Option<Entry>; POSITION_SLOTS],
    next_id: u32,
}

impl Default for PositionTracker {
    fn default() -> Self {
        PositionTracker {
            entries: [None; POSITION_SLOTS],
            next_id: 1,
        }
    }
}

impl PositionTracker {
    pub(crate) fn clear(&mut self, screens: &ScreenSet) {
        for slot in self.entries.iter_mut() {
            if let Some(entry) = slot.take() {
                entry.release(screens);
            }
        }
    }

    pub(crate) fn track(&mut self, term: &mut Terminal, row: u32, col: u32) -> u32 {
        if self.next_id == u32::MAX || col >= u32::from(term.cols) {
            return 0;
        }
        let screen = term.screens.active;
        if screen.is_null() {
            return 0;
        }
        let key = term.screens.active_key;
        let generation = term.screens.generation(key);
        // SAFETY: `active` is the set's live active screen; `&mut Terminal`
        // guarantees nothing else is touching it.
        let pages = unsafe { &mut (*screen).pages };
        let Some(pin) = pages.pin(Point::Screen(Coordinate {
            x: col as u16,
            y: row,
        })) else {
            return 0;
        };
        let Some(slot) = self.entries.iter_mut().find(|s| s.is_none()) else {
            return 0;
        };
        let Ok(tracked) = pages.track_pin(pin) else {
            return 0;
        };
        let id = self.next_id;
        self.next_id += 1;
        *slot = Some(Entry {
            id,
            pin: tracked,
            screen,
            key,
            generation,
        });
        id
    }

    /// `row * 65536 + col` in screen coordinates, or -1.
    pub(crate) fn resolve(&self, term: &Terminal, id: u32) -> f64 {
        let Some(entry) = self.entries.iter().flatten().find(|e| e.id == id) else {
            return -1.0;
        };
        if !entry.live(&term.screens) || entry.screen != term.screens.active {
            return -1.0;
        }
        // SAFETY: the entry is live, so its screen and tracked pin are allocated.
        let (pin, pages) = unsafe { (*entry.pin, &(*entry.screen).pages) };
        if pin.garbage {
            return -1.0;
        }
        match pages.point_from_pin(Tag::Screen, pin) {
            // Both integers fit exactly in an f64 (u32 row, u16 col).
            Some(pt) => (u64::from(pt.coord().y) * 65536 + u64::from(pt.coord().x)) as f64,
            None => -1.0,
        }
    }

    pub(crate) fn release(&mut self, screens: &ScreenSet, id: u32) {
        for slot in self.entries.iter_mut() {
            if slot.is_some_and(|e| e.id == id) {
                if let Some(entry) = slot.take() {
                    entry.release(screens);
                }
                return;
            }
        }
    }
}

fn semantic(pin: &Pin) -> SemanticPrompt {
    // SAFETY: `row_and_cell` returns a live row pointer for a valid pin.
    unsafe { (*pin.row_and_cell().row).semantic_prompt() }
}

fn wrap_continuation(pin: &Pin) -> bool {
    // SAFETY: as in `semantic`.
    unsafe { (*pin.row_and_cell().row).wrap_continuation() }
}

fn prompt_continues(pin: &Pin) -> bool {
    let sp = semantic(pin);
    sp == SemanticPrompt::PromptContinuation
        || (sp != SemanticPrompt::None && wrap_continuation(pin))
}

/// Screen row of the previous (`direction == -1`) or next (`1`) prompt
/// relative to `row`, using the page list's own prompt iterator. Primary
/// screen only; -1 when there is none.
pub(crate) fn find_prompt(term: &Terminal, row: u32, direction: i32) -> f64 {
    find_prompt_row(term, row, direction).map_or(-1.0, f64::from)
}

fn find_prompt_row(term: &Terminal, row: u32, direction: i32) -> Option<u32> {
    if term.screens.active_key != Key::Primary || (direction != -1 && direction != 1) {
        return None;
    }
    let screen = term.screens.active;
    if screen.is_null() {
        return None;
    }
    // SAFETY: the active screen is owned by `term.screens` and outlives this
    // shared borrow of `term`; we only read the page list.
    let pages = unsafe { &(*screen).pages };
    let origin = pages.pin(Point::Screen(Coordinate { x: 0, y: row }))?;
    let start = if direction < 0 {
        origin.up(1)?
    } else {
        let mut pin = origin.down(1)?;
        // Do not treat the remainder of the current multi-line prompt as
        // another prompt when moving forwards.
        if semantic(&origin) != SemanticPrompt::None {
            while prompt_continues(&pin) {
                pin = pin.down(1)?;
            }
        }
        pin
    };
    let dir = if direction < 0 {
        Direction::LeftUp
    } else {
        Direction::RightDown
    };
    let mut prompt = start.prompt_iterator(dir, None).next()?;
    // Reflow can copy `.prompt` to wrapped rows rather than
    // `.prompt_continuation`; resolve those to the logical start.
    while prompt_continues(&prompt) {
        let Some(prior) = prompt.up(1) else { break };
        if semantic(&prior) == SemanticPrompt::None {
            break;
        }
        prompt = prior;
    }
    Some(pages.point_from_pin(Tag::Screen, prompt)?.coord().y)
}
