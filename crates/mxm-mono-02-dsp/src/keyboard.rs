//! The keyboard block: low-note priority, and a trigger that fires only from below.
//!
//! The SH-2's keyboard is a resistor chain on a bus bar — the lowest closed contact
//! wins — and its trigger is a pulse differentiated from the gate's rising edge and
//! from every *downward* change of the key CV (`research:instruments/sh-2.md` §5). The
//! owner's manual spells out the consequences this module reproduces: *"if more than
//! one note is played at a time, the bottom one will be sounded"*; *"new keys
//! depressed on the left will introduce a new trigger while new keys at the right
//! will not"*; releasing the lowest *"will not change the pitch until any keys to its
//! left are released"* — and does so without a trigger.
//!
//! Nothing in the collection had this shape. `mxm-mono-01-dsp`'s stack sounds the
//! **newest** held key; `mxm-poly-06-dsp`'s ledger hands keys to voices. This block
//! answers two questions per event — *which press sounds* (the lowest held) and *did
//! that just open from nothing or change downward* (a trigger) — and the voice wires
//! the trigger to its three consumers: the envelope in GATE+TRIG mode, the LFO
//! delay's restart and the auto bend's restart. A higher key over a held lower one
//! produces **no event at all**, which is why each consumer is tested separately.
//!
//! # Presses, not pitches
//!
//! The block keeps **presses** — one entry per note-on until its release, with the
//! per-note state a host may address to it — because the player's tie emits the
//! new note *before* the old one's release, so for a moment two presses of the same
//! key are held (`apps/mxm-player/src/events/press.rs`). A set keyed by pitch would
//! silence a same-pitch tie or route a bend to a released id while still passing
//! every no-retrigger test. Among presses of equal key the **older** one sounds, so a
//! same-pitch press changes nothing until the older press is released, and the
//! release then hands over with no trigger and no gate change.
//!
//! # Bounded, with exhaustion as a policy
//!
//! Fixed capacity — the realtime rules forbid growing it, and a host can send more
//! overlapping note-ons than any bound without a release. When full, a new press
//! **evicts the oldest press that is not sounding**, retiring it as its release
//! would have (no trigger, no audible change, since it was not sounding). The
//! sounding press is never evicted by a press, so its release always finds it and
//! no note can stick. A release for a press already evicted is unmatched and dropped.
//!
//! # The owner outlives the press
//!
//! The SH-2's key CV holds the last note after every release (§5.2), and this block
//! holds the last sounding press's key, channel and per-note expression the same way:
//! a release tail or a HOLD drone bends with the channel that played it and keeps the
//! offset it was bent to, until a new press sounds. Before the first press since
//! `reset`, the owner is a defined default the voice supplies.

/// How many presses the block can hold at once. Sixteen is more keys than two
/// hands hold; the number matters less than the eviction rule, which is tested.
pub const CAPACITY: usize = 16;

/// A note's identity as a host sends it. Matched by `voice_id` when both sides have
/// one — the only unambiguous identifier when the same key is pressed twice — and
/// by channel and key otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteId {
    pub voice_id: Option<i32>,
    pub channel: u8,
    pub key: u8,
}

impl NoteId {
    /// `mxm-mono-01-dsp`'s rule, carried whole: an id is authoritative when both
    /// sides carry one, otherwise the key decides — so a note-off carrying an id the
    /// press does not carry is not about it, whatever the key says.
    pub fn matches(&self, voice_id: Option<i32>, channel: u8, key: u8) -> bool {
        match (self.voice_id, voice_id) {
            (Some(a), Some(b)) => a == b,
            _ => self.channel == channel && self.key == key,
        }
    }
}

/// One note-on until its release, with the per-note state addressed to it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Press {
    pub id: NoteId,
    /// The note-on's velocity, `0..=1`. The machine's keyboard had none; it is a routable source.
    pub velocity: f32,
    /// The host's per-note pitch expression, in semitones. Zero unless addressed.
    pub expression_semitones: f32,
}

/// What the voice follows: the sounding press, or after the last release the last
/// press that sounded, or before any press the default the voice was built with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Owner {
    pub key: u8,
    pub channel: u8,
    /// The sounding press's velocity, or the last one's; zero before the first press.
    pub velocity: f32,
    pub expression_semitones: f32,
}

/// What one event did. Every field defaults to `false`; an unmatched release is the
/// default outcome, which is how "dropped" is spelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Outcome {
    /// No key was held before this event and one is now.
    pub gate_opened: bool,
    /// A key was held before this event and none is now.
    pub gate_closed: bool,
    /// The trigger fired: the gate opened, or a new *lower* key took over. Never on
    /// a release, never on a higher key, never on a same-pitch press.
    pub trigger: bool,
    /// The sounding press is a different press than before — by a takeover from
    /// below, or by a fallback on release. A same-pitch handoff sets this with the
    /// pitch unchanged, because the owner's channel and expression may differ.
    pub sounding_changed: bool,
    /// A choke removed the sounding press and nothing remains: the voice stops
    /// without its release tail.
    pub cut: bool,
}

/// The keyboard block.
#[derive(Debug, Clone)]
pub struct Keyboard {
    /// Held presses in order of arrival, oldest first, compacted at the front.
    presses: [Option<Press>; CAPACITY],
    len: usize,
    /// The last press that sounded — or the default, before any did.
    owner: Owner,
    default_owner: Owner,
}

impl Keyboard {
    /// `default_key` and `default_channel` are what a HOLD drone sounds with before
    /// the first press; both are the voice's chosen constants, not this module's.
    pub const fn new(default_key: u8, default_channel: u8) -> Self {
        let default_owner = Owner {
            key: default_key,
            channel: default_channel,
            velocity: 0.0,
            expression_semitones: 0.0,
        };
        Self {
            presses: [None; CAPACITY],
            len: 0,
            owner: default_owner,
            default_owner,
        }
    }

    /// Forget every press and return the owner to the default. A fresh start.
    pub fn reset(&mut self) {
        self.presses = [None; CAPACITY];
        self.len = 0;
        self.owner = self.default_owner;
    }

    pub fn is_held(&self) -> bool {
        self.len > 0
    }

    pub fn held(&self) -> usize {
        self.len
    }

    /// The press that sounds: the lowest key held, and among equal keys the oldest.
    pub fn sounding(&self) -> Option<Press> {
        self.sounding_index().map(|i| self.presses[i].unwrap())
    }

    /// Who the voice follows right now — see the module doc's *owner* section.
    pub fn owner(&self) -> Owner {
        self.owner
    }

    fn sounding_index(&self) -> Option<usize> {
        let mut best: Option<(usize, u8)> = None;
        for (i, p) in self.presses[..self.len].iter().enumerate() {
            let key = p.unwrap().id.key;
            // Strictly lower wins; an equal key keeps the older press (lower index).
            if best.is_none_or(|(_, k)| key < k) {
                best = Some((i, key));
            }
        }
        best.map(|(i, _)| i)
    }

    fn set_owner_from(&mut self, i: usize) {
        let p = self.presses[i].unwrap();
        self.owner = Owner {
            key: p.id.key,
            channel: p.id.channel,
            velocity: p.velocity,
            expression_semitones: p.expression_semitones,
        };
    }

    fn remove_at(&mut self, i: usize) {
        for j in i..self.len - 1 {
            self.presses[j] = self.presses[j + 1];
        }
        self.len -= 1;
        self.presses[self.len] = None;
    }

    /// The oldest press that matches, which is how an id-less release of a
    /// same-pitch tie retires the old press and not the new one.
    fn find(&self, voice_id: Option<i32>, channel: u8, key: u8) -> Option<usize> {
        (0..self.len).find(|&i| self.presses[i].unwrap().id.matches(voice_id, channel, key))
    }

    /// A key went down, at a velocity the voice carries as a source and nothing else reads.
    pub fn note_on(&mut self, id: NoteId, velocity: f32) -> Outcome {
        let was_empty = self.len == 0;
        let before = self.sounding_index();

        if self.len == CAPACITY {
            // Evict the oldest press that is not sounding. The sounding press is never
            // evicted by a press, so its release always finds it.
            let sounding = before.unwrap_or(0);
            let victim = if sounding == 0 { 1 } else { 0 };
            self.remove_at(victim);
        }

        self.presses[self.len] = Some(Press {
            id,
            velocity,
            expression_semitones: 0.0,
        });
        self.len += 1;

        let after = self.sounding_index();
        let new_index = self.len - 1;
        let took_over = after == Some(new_index);
        // Eviction may have shifted the previous sounding press's index; compare by
        // identity, not by index.
        let sounding_changed = took_over;
        if sounding_changed {
            self.set_owner_from(new_index);
        }
        Outcome {
            gate_opened: was_empty,
            gate_closed: false,
            trigger: was_empty || took_over,
            sounding_changed,
            cut: false,
        }
    }

    /// A key came up. Unmatched releases are dropped: the default outcome.
    pub fn note_off(&mut self, voice_id: Option<i32>, channel: u8, key: u8) -> Outcome {
        self.retire(voice_id, channel, key, false)
    }

    /// A host's targeted choke: the press is retired **and its sound stopped**. With
    /// other presses held that is a fallback exactly as on release; with none left
    /// the outcome carries `cut`, and the voice silences without its release tail.
    pub fn choke(&mut self, voice_id: Option<i32>, channel: u8, key: u8) -> Outcome {
        self.retire(voice_id, channel, key, true)
    }

    fn retire(&mut self, voice_id: Option<i32>, channel: u8, key: u8, choke: bool) -> Outcome {
        let Some(i) = self.find(voice_id, channel, key) else {
            return Outcome::default();
        };
        let was_sounding = self.sounding_index() == Some(i);
        self.remove_at(i);
        let mut outcome = Outcome::default();
        if was_sounding {
            match self.sounding_index() {
                Some(next) => {
                    self.set_owner_from(next);
                    outcome.sounding_changed = true;
                }
                None => {
                    // The owner stays: the key CV holds the last note.
                    outcome.gate_closed = true;
                    outcome.cut = choke;
                }
            }
        }
        outcome
    }

    /// All Notes Off: every key rising at once. One gate close, no trigger, the
    /// owner left on the last press. Never a panic — that is the voice's All Sound
    /// Off, which this block does not know about.
    pub fn all_notes_off(&mut self) -> Outcome {
        let was_held = self.len > 0;
        self.presses = [None; CAPACITY];
        self.len = 0;
        Outcome {
            gate_closed: was_held,
            ..Outcome::default()
        }
    }

    /// A per-note pitch expression, addressed to a press. Returns whether a press
    /// took it; an expression for a press this block does not hold belongs to no
    /// gate here and is dropped, and so is a non-finite one. If the addressed press
    /// is the sounding one the owner follows it at once.
    pub fn set_expression(
        &mut self,
        voice_id: Option<i32>,
        channel: u8,
        key: u8,
        semitones: f32,
    ) -> bool {
        // A non-finite offset is refused, and the press keeps the one it had: a NaN in the
        // pitch sum would reach both VCOs' phases and never leave.
        if !semitones.is_finite() {
            return false;
        }
        // Prefer the sounding press when it matches: an id-less expression on a
        // same-pitch tie is about the note that is heard.
        let target = match self.sounding_index() {
            Some(s) if self.presses[s].unwrap().id.matches(voice_id, channel, key) => Some(s),
            _ => self.find(voice_id, channel, key),
        };
        let Some(i) = target else {
            return false;
        };
        self.presses[i].as_mut().unwrap().expression_semitones = semitones;
        if self.sounding_index() == Some(i) {
            self.owner.expression_semitones = semitones;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(key: u8) -> NoteId {
        NoteId {
            voice_id: None,
            channel: 0,
            key,
        }
    }

    fn vid(voice_id: i32, key: u8) -> NoteId {
        NoteId {
            voice_id: Some(voice_id),
            channel: 0,
            key,
        }
    }

    fn kb() -> Keyboard {
        Keyboard::new(60, 0)
    }

    #[test]
    fn the_first_key_opens_the_gate_and_triggers() {
        let mut k = kb();
        let o = k.note_on(id(60), 1.0);
        assert!(o.gate_opened && o.trigger && o.sounding_changed);
        assert_eq!(k.sounding().unwrap().id.key, 60);
    }

    #[test]
    fn a_higher_key_over_a_held_lower_key_does_nothing() {
        // Wart 7: no pitch change, no trigger — and, from the review, no change to
        // the held key's expression either.
        let mut k = kb();
        k.note_on(id(48), 1.0);
        assert!(k.set_expression(None, 0, 48, 0.5));
        let o = k.note_on(id(60), 1.0);
        assert_eq!(o, Outcome::default(), "a higher key must produce no event");
        assert_eq!(k.sounding().unwrap().id.key, 48);
        assert_eq!(k.owner().expression_semitones, 0.5);
        assert_eq!(k.held(), 2, "but the key is held, and matters on release");
    }

    #[test]
    fn a_lower_key_takes_over_with_a_trigger() {
        // Wart 8: retrigger only from below.
        let mut k = kb();
        k.note_on(id(60), 1.0);
        let o = k.note_on(id(48), 1.0);
        assert!(o.trigger && o.sounding_changed && !o.gate_opened);
        assert_eq!(k.sounding().unwrap().id.key, 48);
    }

    #[test]
    fn releasing_the_lowest_drops_to_the_next_lowest_without_a_trigger() {
        let mut k = kb();
        k.note_on(id(48), 1.0);
        k.note_on(id(60), 1.0);
        k.note_on(id(55), 1.0);
        let o = k.note_off(None, 0, 48);
        assert!(o.sounding_changed && !o.trigger && !o.gate_closed);
        assert_eq!(k.sounding().unwrap().id.key, 55);
        let o = k.note_off(None, 0, 55);
        assert!(o.sounding_changed && !o.trigger);
        assert_eq!(k.sounding().unwrap().id.key, 60);
        let o = k.note_off(None, 0, 60);
        assert!(o.gate_closed && !o.trigger && !o.sounding_changed && !o.cut);
        assert!(k.sounding().is_none());
    }

    #[test]
    fn releasing_a_higher_held_key_changes_nothing_audible() {
        let mut k = kb();
        k.note_on(id(48), 1.0);
        k.note_on(id(60), 1.0);
        let o = k.note_off(None, 0, 60);
        assert_eq!(o, Outcome::default());
        assert_eq!(k.sounding().unwrap().id.key, 48);
    }

    #[test]
    fn a_same_pitch_tie_hands_off_between_two_presses_without_a_gap() {
        // The player emits the new press before the old one's release.
        let mut k = kb();
        k.note_on(vid(1, 60), 1.0);
        assert!(k.set_expression(Some(1), 0, 60, 0.25));
        // The second press of the same key: no trigger, no change of sounding press.
        let o = k.note_on(vid(2, 60), 1.0);
        assert_eq!(o, Outcome::default());
        assert_eq!(k.sounding().unwrap().id.voice_id, Some(1));
        assert_eq!(
            k.owner().expression_semitones,
            0.25,
            "the old press still owns"
        );
        // An expression addressed to the new press lands on it, not on the owner yet.
        assert!(k.set_expression(Some(2), 0, 60, -1.0));
        assert_eq!(k.owner().expression_semitones, 0.25);
        // The old press's release by id: handoff, no trigger, gate stays open.
        let o = k.note_off(Some(1), 0, 60);
        assert!(o.sounding_changed && !o.trigger && !o.gate_closed);
        assert_eq!(k.sounding().unwrap().id.voice_id, Some(2));
        assert_eq!(
            k.owner().expression_semitones,
            -1.0,
            "now the new press owns"
        );
        assert_eq!(k.owner().key, 60);
        // A stale expression for the released press belongs to no gate here.
        assert!(!k.set_expression(Some(1), 0, 60, 3.0));
        assert_eq!(k.owner().expression_semitones, -1.0);
    }

    #[test]
    fn an_id_less_release_retires_the_oldest_press_of_that_key() {
        // poly-06's rule, for the same reason: the tie's old press is the one being
        // released, and a by-key match on the newest would silence the new note.
        let mut k = kb();
        k.note_on(vid(1, 60), 1.0);
        k.note_on(vid(2, 60), 1.0);
        let o = k.note_off(None, 0, 60);
        assert!(o.sounding_changed && !o.gate_closed);
        assert_eq!(k.sounding().unwrap().id.voice_id, Some(2));
    }

    #[test]
    fn a_note_off_carrying_a_foreign_id_is_not_about_a_held_press() {
        let mut k = kb();
        k.note_on(vid(7, 60), 1.0);
        assert_eq!(k.note_off(Some(8), 0, 60), Outcome::default());
        assert!(k.is_held());
    }

    #[test]
    fn a_stack_keyed_by_pitch_would_fail_the_tie() {
        // The sabotage the plan asks for, expressed as the property a pitch-keyed
        // set cannot have: two presses of one key are two entries.
        let mut k = kb();
        k.note_on(vid(1, 60), 1.0);
        k.note_on(vid(2, 60), 1.0);
        assert_eq!(k.held(), 2);
        k.note_off(Some(1), 0, 60);
        assert!(k.is_held(), "the tie must not go silent");
    }

    #[test]
    fn the_block_survives_exhaustion_without_a_stuck_note() {
        let mut k = kb();
        // A low key first, then more presses above it than the block holds.
        k.note_on(vid(0, 36), 1.0);
        for n in 1..(CAPACITY as i32 + 6) {
            let o = k.note_on(vid(n, 60 + (n % 12) as u8), 1.0);
            assert_eq!(o, Outcome::default(), "presses above the lowest do nothing");
        }
        assert_eq!(k.held(), CAPACITY);
        assert_eq!(
            k.sounding().unwrap().id.voice_id,
            Some(0),
            "the lowest still sounds"
        );
        // A release for an evicted press is dropped.
        assert_eq!(k.note_off(Some(1), 0, 61), Outcome::default());
        // The sounding press's own release still finds it and falls back.
        let o = k.note_off(Some(0), 0, 36);
        assert!(o.sounding_changed && !o.trigger);
        assert!(k.sounding().unwrap().id.key >= 60);
    }

    #[test]
    fn exhaustion_never_evicts_the_sounding_press_even_when_it_is_the_newest() {
        let mut k = kb();
        for n in 0..CAPACITY as i32 {
            k.note_on(vid(n, 72 - n as u8), 1.0); // descending: each press takes over
        }
        let newest = CAPACITY as i32 - 1;
        assert_eq!(k.sounding().unwrap().id.voice_id, Some(newest));
        // Full, and the sounding press is at the *end*. A higher press must evict
        // the oldest, not the sounding one.
        let o = k.note_on(vid(100, 100), 1.0);
        assert_eq!(o, Outcome::default());
        assert_eq!(k.sounding().unwrap().id.voice_id, Some(newest));
        assert_eq!(k.note_off(Some(0), 0, 72), Outcome::default(), "evicted");
    }

    #[test]
    fn all_notes_off_releases_everything_and_keeps_the_owner() {
        let mut k = kb();
        k.note_on(
            NoteId {
                voice_id: None,
                channel: 3,
                key: 50,
            },
            1.0,
        );
        k.set_expression(None, 3, 50, 0.7);
        k.note_on(id(70), 1.0);
        let o = k.all_notes_off();
        assert!(o.gate_closed && !o.trigger && !o.cut);
        assert!(!k.is_held());
        let owner = k.owner();
        assert_eq!(
            (owner.key, owner.channel, owner.expression_semitones),
            (50, 3, 0.7)
        );
        assert_eq!(
            k.all_notes_off(),
            Outcome::default(),
            "nothing held: no event"
        );
    }

    #[test]
    fn choke_is_a_release_with_others_held_and_a_cut_alone() {
        let mut k = kb();
        k.note_on(id(48), 1.0);
        k.note_on(id(60), 1.0);
        // Choking the higher press changes nothing audible.
        assert_eq!(k.choke(None, 0, 60), Outcome::default());
        k.note_on(id(60), 1.0);
        // Choking the sounding press with another held: fallback, not a cut.
        let o = k.choke(None, 0, 48);
        assert!(o.sounding_changed && !o.cut && !o.gate_closed);
        assert_eq!(k.sounding().unwrap().id.key, 60);
        // Choking the last press: gate closed and cut.
        let o = k.choke(None, 0, 60);
        assert!(o.gate_closed && o.cut);
        // The same by note-off would not be a cut.
        k.note_on(id(60), 1.0);
        let o = k.note_off(None, 0, 60);
        assert!(o.gate_closed && !o.cut);
    }

    #[test]
    fn the_owner_outlives_its_press_and_starts_as_the_default() {
        let mut k = Keyboard::new(45, 2);
        let o = k.owner();
        assert_eq!((o.key, o.channel, o.expression_semitones), (45, 2, 0.0));
        k.note_on(
            NoteId {
                voice_id: Some(1),
                channel: 5,
                key: 62,
            },
            1.0,
        );
        k.set_expression(Some(1), 5, 62, 1.5);
        k.note_off(Some(1), 5, 62);
        let o = k.owner();
        assert_eq!((o.key, o.channel, o.expression_semitones), (62, 5, 1.5));
        k.reset();
        let o = k.owner();
        assert_eq!((o.key, o.channel, o.expression_semitones), (45, 2, 0.0));
    }

    #[test]
    fn the_owner_follows_a_fallback_including_its_channel_and_expression() {
        let mut k = kb();
        k.note_on(
            NoteId {
                voice_id: Some(1),
                channel: 1,
                key: 48,
            },
            1.0,
        );
        k.note_on(
            NoteId {
                voice_id: Some(2),
                channel: 2,
                key: 60,
            },
            1.0,
        );
        k.set_expression(Some(2), 2, 60, -0.3);
        assert_eq!(k.owner().channel, 1);
        k.note_off(Some(1), 1, 48);
        let o = k.owner();
        assert_eq!((o.key, o.channel, o.expression_semitones), (60, 2, -0.3));
    }

    #[test]
    fn an_expression_for_a_press_not_held_is_dropped() {
        let mut k = kb();
        k.note_on(id(60), 1.0);
        assert!(!k.set_expression(None, 0, 61, 1.0));
        assert_eq!(k.owner().expression_semitones, 0.0);
    }
}
