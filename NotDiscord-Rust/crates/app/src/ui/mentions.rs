//! One presentation/editor model for mentions. Resolution only borrows the
//! current snapshot; it owns no global user cache and never performs requests.
use super::*;
use egui::text::{LayoutJob, TextFormat};
#[path = "completion.rs"]
pub(super) mod completion;

pub(super) use notdiscord_core::mentions::{broadcast_at, plain_text_at};

pub(super) fn user_tokens() -> &'static regex::Regex {
    static USER: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"<@!?([0-9]{1,22})>").unwrap());
    &USER
}

pub(super) fn person<'a>(
    id: &str,
    explicit: &'a [Person],
    state: &'a AppSnapshot,
) -> Option<&'a Person> {
    explicit
        .iter()
        .chain(state.guild_members.iter().map(|m| &m.user))
        .chain(state.group_members.iter().map(|m| &m.user))
        .chain(state.search_people.iter())
        .chain(state.current_user.iter())
        .chain(state.profile_user.iter())
        .chain(state.relationships.iter().map(|r| &r.user))
        .find(|person| {
            person.id == id
                && (!person.name.is_empty()
                    || !person.display_name.is_empty()
                    || !person.username.is_empty())
        })
}
pub(super) fn person_name(person: &Person) -> &str {
    [&person.name, &person.display_name, &person.username]
        .into_iter()
        .find(|s| !s.trim().is_empty())
        .map(String::as_str)
        .unwrap_or("Usuario")
}
pub(super) fn name<'a>(id: &str, explicit: &'a [Person], state: &'a AppSnapshot) -> &'a str {
    person(id, explicit, state)
        .map(|person|context_name(person,state))
        .unwrap_or("Usuario")
}
fn context_name<'a>(person:&'a Person,state:&'a AppSnapshot)->&'a str{
    if state.selected_guild_id.is_empty(){person_name(person)}else{state.guild_display_name(person)}
}
pub(super) fn readable(text: &str, explicit: &[Person], state: &AppSnapshot) -> String {
    user_tokens()
        .replace_all(text, |capture: &regex::Captures| {
            let whole = capture.get(0).unwrap();
            if plain_text_at(text, whole.start()) && notdiscord_core::parsing::valid_id(&capture[1])
            {
                format!("@{}", name(&capture[1], explicit, state))
            } else {
                whole.as_str().to_owned()
            }
        })
        .into_owned()
}
pub(super) fn format(size: f32) -> TextFormat {
    TextFormat {
        font_id: egui::FontId::proportional(size),
        color: Color32::from_rgb(230, 237, 255),
        background: Color32::from_rgb(61, 78, 166),
        line_height: Some(size * 1.45),
        ..Default::default()
    }
}
pub(super) fn label(ui: &mut egui::Ui, name: &str, size: f32) -> egui::Response {
    styled_label(ui, name, size, Default::default(), true)
}
pub(super) fn styled_label(ui: &mut egui::Ui, name: &str, size: f32, style: notdiscord_core::markdown::Style, navigable: bool) -> egui::Response {
    let (position, galley, response) = egui::Label::new(
        theme::markdown_text(format!("@{name}"), size, format(size).color, style)
            .background_color(format(size).background),
    )
    .selectable(true)
    .sense(egui::Sense::click_and_drag())
    .layout_in_ui(ui);
    if navigable {
        theme::text_link_galley_feedback(ui, &response, position, &galley, format(size).color);
    }
    if !super::chat::register_history_label(ui, &response, position, galley.clone()) {
        egui::text_selection::LabelSelectionState::label_text_selection(
            ui,
            &response,
            position,
            galley,
            ui.visuals().text_color(),
            egui::Stroke::NONE,
        );
    }
    response.on_hover_cursor(if navigable { egui::CursorIcon::PointingHand } else { egui::CursorIcon::Text })
}
pub(super) fn everyone_allowed(state: &AppSnapshot) -> bool {
    notdiscord_core::mentions::everyone_allowed(&state.selected_guild_id, &state.channel_access)
}
pub(super) fn broadcast_note(text: &str, state: &AppSnapshot) -> Option<&'static str> {
    if !notdiscord_core::mentions::has_broadcast(text) {
        return None;
    }
    if state.selected_guild_id.is_empty() {
        Some(
            "@everyone y @here se conservan como texto en MD y grupos; aquí no hay un permiso de servidor que habilite avisos masivos.",
        )
    } else if !everyone_allowed(state) {
        Some(
            "@everyone y @here se enviarán como texto: no tienes un permiso efectivo confirmado para avisar a todo el canal.",
        )
    } else {
        None
    }
}

/// Metadata is evidence of delivery; syntax merely identifies its visible token.
/// A forwarded snapshot, escaped/code token or plain copied name is not a ping.
pub(super) fn highlights_current_user(message: &ChatMessage, state: &AppSnapshot) -> bool {
    if message.forwarded || message.deleted { return false; }
    let Some(current)=state.current_user.as_ref().filter(|person|notdiscord_core::parsing::valid_id(&person.id)) else {return false;};
    if message.mentions_everyone && notdiscord_core::mentions::has_broadcast(&message.content) {return true;}
    message.mentions.iter().any(|person|person.id==current.id)
        && user_tokens().captures_iter(&message.content).any(|capture| {
            let token=capture.get(0).unwrap();
            capture[1]==current.id && plain_text_at(&message.content,token.start())
        })
}

pub(super) fn query(text: &str, cursor: usize) -> Option<(usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    let end = cursor.min(chars.len());
    let start = chars[..end].iter().rposition(|c| *c == '@')?;
    if end - start > 65
        || start > 0
            && !chars[start - 1].is_whitespace()
            && !matches!(chars[start - 1], '(' | '[' | '{' | ',' | ';')
    {
        return None;
    }
    let start_byte = text
        .char_indices()
        .nth(start)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len());
    if !plain_text_at(text, start_byte) {
        return None;
    }
    let query = &chars[start + 1..end];
    if query
        .iter()
        .any(|c| c.is_whitespace() || matches!(c, '<' | '>' | '@'))
    {
        return None;
    }
    Some((start, query.iter().collect()))
}
pub(super) fn choices(query: &str, state: &AppSnapshot, limit: usize) -> Vec<Person> {
    let limit = limit.min(8);
    let query = query.to_lowercase();
    let mut choices = Vec::with_capacity(limit);
    if everyone_allowed(state) {
        for broadcast in ["everyone", "here"] {
            if broadcast.starts_with(&query) && choices.len() < limit {
                choices.push(Person {
                    id: broadcast.into(),
                    name: broadcast.into(),
                    username: broadcast.into(),
                    ..Default::default()
                });
            }
        }
    }
    for person in state
        .search_people
        .iter()
        .chain(state.group_members.iter().map(|m| &m.user))
        .chain(state.current_user.iter())
    {
        if choices.len() >= limit {
            break;
        }
        if notdiscord_core::parsing::valid_id(&person.id)
            && !choices.iter().any(|p| p.id == person.id)
            && (context_name(person,state).to_lowercase().contains(&query)
                || person.username.to_lowercase().contains(&query))
        {
            let mut p = person.clone();
            p.name = context_name(person,state).into();
            choices.push(p);
        }
    }
    choices
}

pub(super) fn insert(draft: &mut Draft, start: usize, end: usize, person: &Person) {
    let label = format!("@{}", person_name(person));
    let mut chars: Vec<char> = draft.text.chars().collect();
    if start > end || end > chars.len() {
        return;
    }
    chars.splice(start..end, label.chars().chain([' ']));
    let old = std::mem::replace(&mut draft.text, chars.iter().collect());
    adjust(&old, &draft.text, &mut draft.mentions);
    if notdiscord_core::parsing::valid_id(&person.id) {
        draft.mentions.push(Mention {
            start,
            end: start + label.chars().count(),
            id: person.id.clone(),
            label,
        });
    }
    draft.revision += 1;
}
pub(super) fn adjust(old: &str, new: &str, spans: &mut Vec<Mention>) {
    let a: Vec<char> = old.chars().collect();
    let b: Vec<char> = new.chars().collect();
    let prefix = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let old_end = a.len() - suffix;
    let delta = b.len() as isize - a.len() as isize;
    let inserted = b.len() - prefix - suffix;
    // Most TextEdit updates contain one edit and the prefix/suffix fast path
    // is sufficient. A single frame may also prepend AND append text (or undo
    // several operations): treating that as one replacement discarded intact
    // mentions in its middle. Recover only unchanged, contiguous character
    // runs from a bounded edit script; never reverse-resolve visible names.
    let runs = spans
        .iter()
        .any(|span| span.start < old_end && span.end > prefix)
        .then(|| unchanged_runs(&a, &b))
        .flatten();
    spans.retain_mut(|m| {
        if m.start >= old_end {
            m.start = (m.start as isize + delta).max(0) as usize;
            m.end = (m.end as isize + delta).max(0) as usize;
        } else if m.end <= prefix {
            if prefix == m.end && inserted > 0 {
                let c = b[prefix];
                if !c.is_whitespace() && !is_punctuation(c) {
                    return false;
                }
            }
        } else if let Some((old_start, new_start, _)) = runs.as_ref().and_then(|runs| {
            runs.iter()
                .find(|(start, _, len)| m.start >= *start && m.end <= *start + *len)
        }) {
            let end = new_start + (m.end - old_start);
            // Extending the end of a name with a word character is an edit of
            // that name, even when all its original characters still match.
            let next_unchanged = runs.as_ref().is_some_and(|runs| {
                runs.iter().any(|(start, target, len)| {
                    m.end >= *start && m.end < *start + *len && target + (m.end - start) == end
                })
            });
            if !next_unchanged
                && b.get(end)
                    .is_some_and(|c| !c.is_whitespace() && !is_punctuation(*c))
            {
                return false;
            }
            m.start = new_start + (m.start - old_start);
            m.end = end;
        } else {
            return false;
        }
        b.get(m.start..m.end)
            .is_some_and(|s| s.iter().collect::<String>() == m.label)
    });
}

/// Myers' shortest-edit frontier, with strict transient work/memory limits.
/// Returns the exact matching runs, not guesses based on repeated labels.
fn unchanged_runs(a: &[char], b: &[char]) -> Option<Vec<(usize, usize, usize)>> {
    const MAX_EDITS: usize = 64;
    const MAX_WORK: usize = 1_000_000;
    let offset = MAX_EDITS + 1;
    let mut frontier = vec![-1isize; MAX_EDITS * 2 + 3];
    frontier[offset + 1] = 0;
    let mut trace = Vec::with_capacity(MAX_EDITS + 1);
    let mut work = 0;
    for distance in 0..=MAX_EDITS {
        trace.push(frontier.clone());
        for diagonal in (-(distance as isize)..=distance as isize).step_by(2) {
            let index = (offset as isize + diagonal) as usize;
            let mut x = if diagonal == -(distance as isize)
                || diagonal != distance as isize && frontier[index - 1] < frontier[index + 1]
            {
                frontier[index + 1]
            } else {
                frontier[index - 1] + 1
            };
            let mut y = x - diagonal;
            while x >= 0
                && y >= 0
                && (x as usize) < a.len()
                && (y as usize) < b.len()
                && a[x as usize] == b[y as usize]
            {
                x += 1;
                y += 1;
                work += 1;
                if work > MAX_WORK {
                    return None;
                }
            }
            work += 1;
            if work > MAX_WORK {
                return None;
            }
            frontier[index] = x;
            if x as usize >= a.len() && y as usize >= b.len() {
                let (mut x, mut y) = (a.len() as isize, b.len() as isize);
                let mut runs = Vec::with_capacity(distance + 1);
                for depth in (0..=distance).rev() {
                    let previous = &trace[depth];
                    let diagonal = x - y;
                    let index = (offset as isize + diagonal) as usize;
                    let previous_diagonal = if diagonal == -(depth as isize)
                        || diagonal != depth as isize && previous[index - 1] < previous[index + 1]
                    {
                        diagonal + 1
                    } else {
                        diagonal - 1
                    };
                    let previous_x = previous[(offset as isize + previous_diagonal) as usize];
                    let previous_y = previous_x - previous_diagonal;
                    let len = (x - previous_x).min(y - previous_y).max(0);
                    if len > 0 {
                        runs.push(((x - len) as usize, (y - len) as usize, len as usize));
                    }
                    x = previous_x;
                    y = previous_y;
                }
                return Some(runs);
            }
        }
    }
    None
}
fn is_punctuation(c: char) -> bool {
    static PUNCTUATION: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PUNCTUATION
        .get_or_init(|| regex::Regex::new(r"^\p{P}$").unwrap())
        .is_match(&c.to_string())
}
pub(super) fn outgoing(draft: &Draft) -> String {
    let mut chars: Vec<char> = draft.text.chars().collect();
    let mut mentions = draft.mentions.clone();
    mentions.sort_by_key(|m| std::cmp::Reverse(m.start));
    for m in mentions {
        if notdiscord_core::parsing::valid_id(&m.id)
            && char_byte(&draft.text, m.start).is_some_and(|start| plain_text_at(&draft.text, start))
            && chars
                .get(m.start..m.end)
                .is_some_and(|s| s.iter().collect::<String>() == m.label)
        {
            chars.splice(m.start..m.end, format!("<@{}>", m.id).chars());
        }
    }
    chars.iter().collect()
}
pub(super) fn from_source(source: &str, explicit: &[Person], state: &AppSnapshot) -> Draft {
    let mut draft = Draft::default();
    let mut end = 0;
    for capture in user_tokens().captures_iter(source) {
        let whole = capture.get(0).unwrap();
        if !plain_text_at(source, whole.start()) || !notdiscord_core::parsing::valid_id(&capture[1])
        {
            continue;
        }
        draft.text.push_str(&source[end..whole.start()]);
        let start = draft.text.chars().count();
        let label = format!("@{}", name(&capture[1], explicit, state));
        draft.text.push_str(&label);
        draft.mentions.push(Mention {
            start,
            end: start + label.chars().count(),
            id: capture[1].into(),
            label,
        });
        end = whole.end();
    }
    draft.text.push_str(&source[end..]);
    draft
}

/// Re-resolve intact spans when existing snapshot metadata arrives. No name-
/// based reverse lookup is ever used: two identical names keep separate IDs.
pub(super) fn refresh_names(draft: &mut Draft, explicit: &[Person], state: &AppSnapshot) -> bool {
    let mut changed = false;
    let mut spans = draft.mentions.clone();
    spans.sort_by_key(|m| std::cmp::Reverse(m.start));
    for span in spans {
        let Some(person) = person(&span.id, explicit, state) else {
            continue;
        };
        let label = format!("@{}", context_name(person,state));
        if label == span.label {
            continue;
        }
        let mut chars: Vec<char> = draft.text.chars().collect();
        if chars
            .get(span.start..span.end)
            .is_none_or(|s| s.iter().collect::<String>() != span.label)
        {
            continue;
        }
        chars.splice(span.start..span.end, label.chars());
        let old = std::mem::replace(&mut draft.text, chars.iter().collect());
        adjust(&old, &draft.text, &mut draft.mentions);
        draft.mentions.push(Mention {
            start: span.start,
            end: span.start + label.chars().count(),
            id: span.id,
            label,
        });
        changed = true;
    }
    changed
}

/// TextEdit paints these ranges using the same font, baseline and ink as chat.
/// The editable buffer contains names; span metadata serializes their IDs.
pub(super) fn editor_job(
    text: &str,
    spans: &[Mention],
    size: f32,
    ink: Color32,
    allow_broadcast: bool,
    wrap_width: f32,
) -> LayoutJob {
    let mut ranges: Vec<std::ops::Range<usize>> = spans
        .iter()
        .filter_map(|span| {
            let start = char_byte(text, span.start)?;
            let end = char_byte(text, span.end)?;
            (text.get(start..end) == Some(span.label.as_str())).then_some(start..end)
        })
        .collect();
    if allow_broadcast {
        for (start, _) in text.match_indices('@') {
            for token in ["@everyone", "@here"] {
                let end = start + token.len();
                if text[start..].starts_with(token) && broadcast_at(text, start, end) {
                    ranges.push(start..end);
                }
            }
        }
    }
    ranges.sort_by_key(|range| range.start);
    // Mention display names are metadata atoms, even if a name contains * or
    // _. Mask only the parser's recognition copy with equal-length ASCII; all
    // ranges and all emitted characters still refer to the original buffer.
    let mut recognition = text.to_owned();
    for range in ranges.iter().rev() {
        if recognition.get(range.clone()).is_some() {
            recognition.replace_range(range.clone(), &"x".repeat(range.len()));
        }
    }
    let document = notdiscord_core::markdown::parse(&recognition);
    let plain = theme::markdown_format(Default::default(), size, ink, false);
    let mut base = LayoutJob::default();
    let mut end = 0;
    for block in &document.blocks {
        let content_size = match &block.kind {
            notdiscord_core::markdown::BlockKind::Heading { level } => size * match level { 1 => 1.6, 2 => 1.35, _ => 1.15 },
            notdiscord_core::markdown::BlockKind::Subtext => (size * 0.85).max(11.),
            _ => size,
        };
        for span in &block.spans {
            if span.range.start < end { continue; }
            let Some(value) = text.get(span.range.clone()) else { continue; };
            if span.range.start > end { base.append(&text[end..span.range.start], 0., plain.clone()); }
            base.append(value, 0., theme::markdown_format(span.style, if span.syntax { size } else { content_size }, ink, span.syntax));
            end = span.range.end;
        }
    }
    base.append(&text[end..], 0., plain);
    // Overlay mention colours without replacing the Markdown font or changing
    // a single source character: TextEdit caret/selection offsets stay exact.
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;
    let mut range_index = 0;
    for section in base.sections {
        let mut cursor = section.byte_range.start;
        while cursor < section.byte_range.end {
            while range_index < ranges.len() && ranges[range_index].end <= cursor { range_index += 1; }
            let mention = ranges.get(range_index);
            let highlighted = mention.is_some_and(|range| range.start <= cursor && cursor < range.end);
            let next = mention.map_or(section.byte_range.end, |range|
                if highlighted { range.end.min(section.byte_range.end) } else { range.start.min(section.byte_range.end) });
            if next <= cursor { break; }
            let mut text_format = section.format.clone();
            if highlighted && text_format.font_id.family != egui::FontFamily::Monospace {
                text_format.color = format(size).color;
                text_format.background = format(size).background;
            }
            job.append(&text[cursor..next], 0., text_format);
            cursor = next;
        }
    }
    job
}
fn char_byte(text: &str, index: usize) -> Option<usize> {
    if index == text.chars().count() {
        Some(text.len())
    } else {
        text.char_indices().nth(index).map(|(byte, _)| byte)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> AppSnapshot {
        AppSnapshot {
            search_people: vec![Person {
                id: "42".into(),
                name: "昵称 Ána".into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }
    #[test]
    fn editing_display_names_round_trips_ids_even_when_names_collide_or_are_missing() {
        let state = state();
        let source = "Hello <@42> <@!43> <@44>";
        let explicit = [Person {
            id: "43".into(),
            name: "昵称 Ána".into(),
            ..Default::default()
        }];
        let mut draft = from_source(source, &explicit, &state);
        assert_eq!(draft.text, "Hello @昵称 Ána @昵称 Ána @Usuario");
        assert_eq!(outgoing(&draft), "Hello <@42> <@43> <@44>");
        let old = draft.text.clone();
        draft.text = format!("前 {old}!");
        adjust(&old, &draft.text, &mut draft.mentions);
        assert_eq!(outgoing(&draft), "前 Hello <@42> <@43> <@44>!");
        let mut newer = state.clone();
        newer.search_people.push(Person {
            id: "44".into(),
            name: "Resolved".into(),
            ..Default::default()
        });
        assert!(refresh_names(&mut draft, &explicit, &newer));
        assert!(draft.text.ends_with("@Resolved!"));
        assert_eq!(outgoing(&draft), "前 Hello <@42> <@43> <@44>!");
        assert!(!refresh_names(&mut draft, &explicit, &newer));
    }
    #[test]
    fn deleting_or_retyping_a_name_does_not_invent_a_mention_from_plain_text() {
        let mut draft = from_source("<@42>", &[], &state());
        let old = draft.text.clone();
        draft.text = "@nickname".into();
        adjust(&old, &draft.text, &mut draft.mentions);
        assert!(draft.mentions.is_empty());
        assert_eq!(outgoing(&draft), "@nickname");
        assert_eq!(
            readable("`<@42>` \\<@42> <@42>", &[], &state()),
            "`<@42>` \\<@42> @昵称 Ána"
        );
    }
    #[test]
    fn server_nicknames_update_mentions_and_intact_drafts_without_changing_target_ids(){
        let mut state=state();state.selected_guild_id="10".into();
        state.search_people[0].display_name="Global".into();state.search_people[0].username="Account".into();
        state.guild_nicknames.insert("42".into(),"Server nickname".into());
        assert_eq!(readable("<@42>",&[],&state),"@Server nickname");
        assert_eq!(choices("server",&state,8)[0].name,"Server nickname");
        let mut draft=from_source("Hi <@42>",&[],&state);
        state.guild_nicknames.insert("42".into(),"Updated nickname".into());
        assert!(refresh_names(&mut draft,&[],&state));assert_eq!(draft.text,"Hi @Updated nickname");assert_eq!(outgoing(&draft),"Hi <@42>");
        state.guild_nicknames.insert("42".into(),String::new());assert!(refresh_names(&mut draft,&[],&state));assert_eq!(draft.text,"Hi @Global");assert_eq!(outgoing(&draft),"Hi <@42>");
        state.selected_guild_id.clear();assert_eq!(readable("<@42>",&[],&state),"@昵称 Ána");
    }
    #[test]
    fn several_edits_in_one_update_preserve_only_intact_mentions() {
        let state = state();
        let explicit = [Person {
            id: "43".into(),
            name: "Second".into(),
            ..Default::default()
        }];
        let mut draft = from_source("left <@42> middle <@43> right", &explicit, &state);
        let old = draft.text.clone();
        draft.text = format!("前 {}!", old.replace("middle", "between"));
        adjust(&old, &draft.text, &mut draft.mentions);
        assert_eq!(outgoing(&draft), "前 left <@42> between <@43> right!");
        let old = draft.text.clone();
        draft.text = format!("prefix {} suffix", old.replace("@Second", "@SecondX"));
        adjust(&old, &draft.text, &mut draft.mentions);
        assert_eq!(
            outgoing(&draft),
            "prefix 前 left <@42> between @SecondX right! suffix"
        );
        assert_eq!(draft.mentions.len(), 1);
        // Wholesale replacement falls back conservatively after the bounded
        // edit search, rather than retaining IDs by matching visible names.
        let old = draft.text.clone();
        draft.text = "changed ".repeat(4096);
        adjust(&old, &draft.text, &mut draft.mentions);
        assert!(draft.mentions.is_empty());
    }
    #[test]
    fn broadcast_completion_and_editor_highlight_use_the_same_effective_policy() {
        let mut state = state();
        assert!(
            !choices("ever", &state, 8)
                .iter()
                .any(|p| p.id == "everyone")
        );
        state.selected_guild_id = "10".into();
        state.channel_access = ChannelAccess::private();
        state.channel_access.can_mention_everyone = true;
        let everyone = choices("ever", &state, 8).remove(0);
        let mut draft = Draft {
            text: "@ever".into(),
            ..Default::default()
        };
        insert(&mut draft, 0, 5, &everyone);
        assert_eq!(outgoing(&draft), "@everyone ");
        assert!(draft.mentions.is_empty());
        let job = editor_job(
            &draft.text,
            &[],
            17.,
            Color32::WHITE,
            everyone_allowed(&state),
            300.,
        );
        assert!(
            job.sections
                .iter()
                .any(|s| s.format.background != Color32::TRANSPARENT)
        );
        assert!(query("`@ever", 6).is_none());
        assert!(query(r"\@ever", 6).is_none());
        assert_eq!(query("é @ever", 7), Some((2, "ever".into())));
        assert_eq!(choices("", &state, usize::MAX).len(), 3);
    }
}
