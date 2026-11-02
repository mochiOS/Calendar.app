use crate::date::{Date, WEEKDAYS, parse_time, time_label};
use crate::store::{Event, Store, storage_path};
use appcore::prelude::*;
use appcore::viewkit::event::{EventContext, EventResult, ViewEvent};
use appcore::viewkit::platform::Key;
use appcore::viewkit::view::{Constraints, MeasureContext, PaintContext};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Action {
    Previous,
    Next,
    Today,
    Select(Date),
    New,
    Edit(u64),
    Save,
    Cancel,
    Delete,
    ConfirmDelete,
}
struct Model {
    month: Date,
    selected: Date,
    today: Date,
    store: Option<Store>,
    error: Option<String>,
    last_clock: Instant,
}
struct Core {
    model: RefCell<Model>,
    action: Rc<Cell<Option<Action>>>,
    toolbar: ActionToolbarState<Action>,
    buttons: RefCell<BTreeMap<Action, ButtonInteractionState>>,
    agenda_scroll: ScrollState,
    days: RefCell<BTreeMap<Date, ButtonInteractionState>>,
    rows: RefCell<BTreeMap<u64, ButtonInteractionState>>,
    editor: RefCell<Option<Editor>>,
}
#[derive(Clone)]
pub struct CalendarView {
    core: Rc<Core>,
}

fn button(label: &str, action: Action, pending: &Rc<Cell<Option<Action>>>) -> Button {
    let pending = pending.clone();
    Button::new(label)
        .size(ButtonSize::Small)
        .on_click(move || pending.set(Some(action)))
}
impl CalendarView {
    pub fn new() -> Self {
        let loaded = storage_path().and_then(Store::load);
        Self::with_store(loaded)
    }
    fn with_store(loaded: Result<Store, String>) -> Self {
        let today = Date::today().unwrap_or(Date {
            year: 1970,
            month: 1,
            day: 1,
        });
        let (store, error) = match loaded {
            Ok(store) => (Some(store), None),
            Err(error) => (None, Some(error)),
        };
        let action = Rc::new(Cell::new(None));
        Self {
            core: Rc::new(Core {
                model: RefCell::new(Model {
                    month: Date { day: 1, ..today },
                    selected: today,
                    today,
                    store,
                    error,
                    last_clock: Instant::now(),
                }),
                action,
                toolbar: ActionToolbarState::new(),
                buttons: RefCell::new(BTreeMap::new()),
                agenda_scroll: ScrollState::new(),
                days: RefCell::new(BTreeMap::new()),
                rows: RefCell::new(BTreeMap::new()),
                editor: RefCell::new(None),
            }),
        }
    }
    fn day_button(
        &self,
        date: Date,
        month: u8,
        selected: Date,
        today: Date,
        events: &[Event],
    ) -> Button {
        let interaction = self.core.days.borrow_mut().entry(date).or_default().clone();
        let action = self.core.action.clone();
        let entries = events
            .iter()
            .take(3)
            .map(|e| e.title.clone())
            .collect::<Vec<_>>();
        Button::with_interaction_and_label(interaction, "")
            .style(ButtonStyle::Ghost)
            .radius(CornerRadius::Small)
            .content_padding(false)
            .content(DayCell {
                date,
                outside: date.month != month,
                selected: date == selected,
                today: date == today,
                titles: entries.clone(),
                event_count: events.len(),
            })
            .accessibility_selected(date == selected)
            .accessibility_label(format!(
                "{date}, {} events{}{}",
                events.len(),
                if date == today { ", today" } else { "" },
                if date == selected { ", selected" } else { "" }
            ))
            .on_click(move || action.set(Some(Action::Select(date))))
    }
    fn row_button(&self, event: &Event, index: usize, count: usize) -> ListRow {
        let interaction = self
            .core
            .rows
            .borrow_mut()
            .entry(event.id)
            .or_default()
            .clone();
        let action = self.core.action.clone();
        let id = event.id;
        let detail = if event.location.is_empty() {
            event_time(event)
        } else {
            format!("{} · {}", event_time(event), event.location)
        };
        ListRow::with_interaction(interaction, &event.title)
            .subtitle(detail)
            .adjacent_rows(index > 0, index + 1 < count)
            .on_select(move || action.set(Some(Action::Edit(id))))
    }
    fn toolbar(&self) -> ActionToolbar<Action> {
        let model = self.core.model.borrow();
        let make = |label: &str, action: Action| {
            let interaction = self
                .core
                .buttons
                .borrow_mut()
                .entry(action)
                .or_default()
                .clone();
            let pending = self.core.action.clone();
            Button::with_interaction_and_label(interaction, label)
                .on_click(move || pending.set(Some(action)))
        };
        ActionToolbar::new(self.core.toolbar.clone())
            .title(model.month.month_title())
            .leading(
                Action::Previous,
                make("", Action::Previous)
                    .leading_symbol(SymbolName::ChevronLeft)
                    .accessibility_label("Previous month"),
            )
            .leading(
                Action::Next,
                make("", Action::Next)
                    .leading_symbol(SymbolName::ChevronRight)
                    .accessibility_label("Next month"),
            )
            .trailing(Action::Today, make("Today", Action::Today))
            .trailing(
                Action::New,
                make("New Event", Action::New)
                    .leading_symbol(SymbolName::Plus)
                    .style(ButtonStyle::Primary)
                    .enabled(model.store.is_some()),
            )
    }
    fn agenda(&self, theme: &Theme) -> VStack {
        let model = self.core.model.borrow();
        let count = model
            .store
            .as_ref()
            .map(|store| store.on_date(model.selected).len())
            .unwrap_or(0);
        let title = format!(
            "{}, {} {}",
            WEEKDAYS[model.selected.weekday()],
            crate::date::MONTHS[model.selected.month as usize - 1],
            model.selected.day
        );
        let subtitle = model.error.clone().unwrap_or_else(|| {
            if count == 0 {
                "No events. Add one with New Event.".into()
            } else if count == 1 {
                "1 event".into()
            } else {
                format!("{count} events")
            }
        });
        VStack::new()
            .alignment(StackAlignment::Stretch)
            .gap(StackGap::None)
            .child(
                Padding::all(theme.spacing.large)
                    .content(
                        VStack::new()
                            .alignment(StackAlignment::Stretch)
                            .gap(StackGap::Small)
                            .child(Text::styled(title, TextRole::TitleSmall))
                            .child(Text::caption(subtitle).color(if model.error.is_some() {
                                theme.colors.destructive
                            } else {
                                theme.colors.text_secondary
                            })),
                    )
                    .layout()
                    .flex_shrink(0.0),
            )
            .child(
                AgendaViewport(self.clone())
                    .layout()
                    .flex_grow(1.0)
                    .flex_shrink(1.0),
            )
    }
    fn apply_action(&self) {
        let Some(action) = self.core.action.take() else {
            return;
        };
        match action {
            Action::Previous | Action::Next => {
                let mut model = self.core.model.borrow_mut();
                if let Some(month) =
                    model
                        .month
                        .shift_month(if matches!(action, Action::Previous) {
                            -1
                        } else {
                            1
                        })
                {
                    model.month = month;
                    model.selected = Date::new(
                        month.year,
                        month.month,
                        model
                            .selected
                            .day
                            .min(Date::days_in_month(month.year, month.month)),
                    )
                    .unwrap();
                    self.core.agenda_scroll.reset();
                    self.core.days.borrow_mut().clear();
                    self.core.rows.borrow_mut().clear();
                }
            }
            Action::Today | Action::Select(_) => {
                let mut model = self.core.model.borrow_mut();
                let date = if let Action::Select(date) = action {
                    date
                } else {
                    Date::today().unwrap_or(model.today)
                };
                model.selected = date;
                model.month = Date { day: 1, ..date };
                self.core.agenda_scroll.reset();
                let grid = model.month.month_grid();
                self.core
                    .days
                    .borrow_mut()
                    .retain(|day, _| grid.contains(&Some(*day)));
                self.core.rows.borrow_mut().clear();
            }
            Action::New | Action::Edit(_) => {
                let model = self.core.model.borrow();
                let Some(store) = model.store.as_ref() else {
                    return;
                };
                let event = if let Action::Edit(id) = action {
                    store.events.iter().find(|e| e.id == id).cloned()
                } else {
                    None
                };
                *self.core.editor.borrow_mut() =
                    Some(Editor::new(model.selected, event, &self.core.action));
            }
            Action::Cancel => {
                let mut editor = self.core.editor.borrow_mut();
                if let Some(editor) = editor.as_mut().filter(|editor| editor.confirm_delete) {
                    editor.confirm_delete = false;
                } else {
                    editor.take();
                }
            }
            Action::Delete => {
                if let Some(editor) = self.core.editor.borrow_mut().as_mut() {
                    editor.confirm_delete = true;
                }
            }
            Action::Save | Action::ConfirmDelete => {
                let mut editor_slot = self.core.editor.borrow_mut();
                let Some(editor) = editor_slot.as_mut() else {
                    return;
                };
                let mut model = self.core.model.borrow_mut();
                let Some(store) = model.store.as_mut() else {
                    return;
                };
                let event = if matches!(action, Action::Save) {
                    match editor.event(store.next_id()) {
                        Ok(event) => Some(event),
                        Err(error) => {
                            editor.error = Some(error);
                            return;
                        }
                    }
                } else {
                    None
                };
                let mut events = store.events.clone();
                if let Some(id) = editor.id {
                    events.retain(|e| e.id != id);
                }
                if let Some(event) = event.as_ref() {
                    events.push(event.clone());
                }
                if let Err(error) = store.save(events) {
                    editor.error = Some(error);
                    return;
                }
                if let Some(event) = event {
                    model.selected = event.date;
                    model.month = Date {
                        day: 1,
                        ..event.date
                    };
                }
                model.error = None;
                self.core.agenda_scroll.reset();
                drop(model);
                drop(editor_slot);
                self.core.editor.borrow_mut().take();
                self.core.rows.borrow_mut().clear();
                self.core.days.borrow_mut().clear();
            }
        }
    }
}

struct Frames {
    toolbar: Rect,
    month: Rect,
    agenda: Rect,
}
impl Frames {
    fn new(bounds: Rect, theme: &Theme) -> Self {
        let toolbar = Rect::new(
            bounds.origin.x,
            bounds.origin.y,
            bounds.size.width,
            theme.layout.top_bar_height,
        );
        let y = toolbar.origin.y + toolbar.size.height;
        let height = (bounds.size.height - toolbar.size.height).max(0.0);
        if bounds.size.width >= 760.0 {
            let side = (bounds.size.width * 0.3).clamp(280.0, 360.0);
            Self {
                toolbar,
                month: Rect::new(
                    bounds.origin.x,
                    y,
                    (bounds.size.width - side).max(0.0),
                    height,
                ),
                agenda: Rect::new(bounds.origin.x + bounds.size.width - side, y, side, height),
            }
        } else {
            let top = (height * 0.60).max(0.0);
            Self {
                toolbar,
                month: Rect::new(bounds.origin.x, y, bounds.size.width, top),
                agenda: Rect::new(
                    bounds.origin.x,
                    y + top,
                    bounds.size.width,
                    (height - top).max(0.0),
                ),
            }
        }
    }
    fn grid_cell(&self, index: usize) -> Rect {
        let x = self.month.origin.x + 16.0;
        let y = self.month.origin.y + 32.0;
        let width = (self.month.size.width - 32.0).max(0.0) / 7.0;
        let height = (self.month.size.height - 48.0).max(0.0) / 6.0;
        Rect::new(
            x + (index % 7) as f32 * width,
            y + (index / 7) as f32 * height,
            (width - 2.0).max(0.0),
            (height - 2.0).max(0.0),
        )
    }
}

impl View for CalendarView {
    fn measure(&self, constraints: Constraints, _: &mut MeasureContext<'_>) -> Size {
        constraints.constrain(constraints.maximum)
    }
    fn paint(&self, bounds: Rect, context: &mut PaintContext<'_>) {
        let toolbar = self.toolbar();
        let mut model = self.core.model.borrow_mut();
        if model.last_clock.elapsed() >= Duration::from_secs(60) {
            if let Some(today) = Date::today() {
                model.today = today;
            }
            model.last_clock = Instant::now();
        }
        context.request_redraw_at(model.last_clock + Duration::from_secs(60));
        let frames = Frames::new(bounds, context.theme);
        Surface::app().paint(bounds, context);
        Toolbar::new(toolbar).paint(frames.toolbar, context);
        Surface::sidebar().paint(frames.month, context);
        Surface::pane()
            .in_container(bounds)
            .paint_content(frames.month, context, |context| {
                for (index, weekday) in WEEKDAYS.iter().enumerate() {
                    let cell = frames.grid_cell(index);
                    Text::caption(*weekday)
                        .color(context.theme.colors.text_secondary)
                        .paint(
                            Rect::new(
                                cell.origin.x + 8.0,
                                frames.month.origin.y + 8.0,
                                cell.size.width,
                                20.0,
                            ),
                            context,
                        );
                }
                for (index, date) in model.month.month_grid().iter().enumerate() {
                    if let Some(date) = date {
                        self.day_button(
                            *date,
                            model.month.month,
                            model.selected,
                            model.today,
                            model
                                .store
                                .as_ref()
                                .map(|store| store.on_date(*date))
                                .unwrap_or(&[]),
                        )
                        .paint(frames.grid_cell(index), context);
                    }
                }
            });
        drop(model);
        Surface::sidebar()
            .content(self.agenda(context.theme))
            .paint(frames.agenda, context);
        if let Some(editor) = self.core.editor.borrow().as_ref() {
            editor.paint(bounds, context);
        }
    }
    fn handle_event(
        &self,
        bounds: Rect,
        event: &ViewEvent,
        context: &mut EventContext<'_>,
    ) -> EventResult {
        if self.core.editor.borrow().is_some() {
            {
                let editor = self.core.editor.borrow();
                editor
                    .as_ref()
                    .unwrap()
                    .handle_event(bounds, event, context);
            }
            if matches!(
                event,
                ViewEvent::KeyPressed {
                    key: Key::Escape,
                    ..
                }
            ) {
                self.core.action.set(Some(Action::Cancel));
            }
            self.apply_action();
            context.request_redraw_in(bounds);
            return EventResult::Consumed;
        }
        let frames = Frames::new(bounds, context.theme());
        let mut result = Toolbar::new(self.toolbar()).handle_event(frames.toolbar, event, context);
        let model = self.core.model.borrow();
        result = result.merge(Surface::pane().in_container(bounds).handle_content_event(
            frames.month,
            event,
            context,
            |event, context| {
                let mut result = EventResult::Ignored;
                for (index, date) in model.month.month_grid().iter().enumerate() {
                    if let Some(date) = date {
                        result = result.merge(
                            self.day_button(
                                *date,
                                model.month.month,
                                model.selected,
                                model.today,
                                model
                                    .store
                                    .as_ref()
                                    .map(|store| store.on_date(*date))
                                    .unwrap_or(&[]),
                            )
                            .handle_event(
                                frames.grid_cell(index),
                                event,
                                context,
                            ),
                        );
                    }
                }
                result
            },
        ));
        drop(model);
        result = result.merge(
            Surface::sidebar()
                .content(self.agenda(context.theme()))
                .handle_event(frames.agenda, event, context),
        );
        match event {
            ViewEvent::KeyPressed { key, modifiers } => {
                let model = self.core.model.borrow();
                let selected = match key {
                    Key::ArrowLeft => model.selected.add_days(-1),
                    Key::ArrowRight => model.selected.add_days(1),
                    Key::ArrowUp => model.selected.add_days(-7),
                    Key::ArrowDown => model.selected.add_days(7),
                    _ => None,
                };
                let action = if let Some(date) = selected {
                    Some(Action::Select(date))
                } else {
                    match key {
                        Key::Home => Some(Action::Today),
                        Key::PageUp => Some(Action::Previous),
                        Key::PageDown => Some(Action::Next),
                        Key::Character('n' | 'N') if modifiers.shortcut() => Some(Action::New),
                        _ => None,
                    }
                };
                if let Some(action) = action {
                    self.core.action.set(Some(action));
                    result = EventResult::Consumed;
                }
            }
            _ => {}
        }
        if self.core.action.get().is_some() {
            self.apply_action();
            context.request_redraw_in(bounds);
        }
        result
    }
}

struct DayCell {
    date: Date,
    outside: bool,
    selected: bool,
    today: bool,
    titles: Vec<String>,
    event_count: usize,
}
impl View for DayCell {
    fn measure(&self, c: Constraints, _: &mut MeasureContext<'_>) -> Size {
        c.constrain(c.maximum)
    }
    fn paint(&self, bounds: Rect, context: &mut PaintContext<'_>) {
        if self.selected {
            Rectangle::new()
                .radius(CornerRadius::Small)
                .color(RectangleColor::Custom(if context.window_active() {
                    context.theme.list.selected_background
                } else {
                    context.theme.list.inactive_selected_background
                }))
                .paint(bounds, context);
        }
        let color = if self.selected {
            if context.window_active() {
                context.theme.list.selected_foreground
            } else {
                context.theme.list.inactive_selected_foreground
            }
        } else if self.today {
            context.theme.colors.accent
        } else if self.outside {
            context.theme.colors.text_tertiary
        } else {
            context.theme.colors.text_primary
        };
        Text::label(self.date.day.to_string())
            .weight(if self.today || self.selected {
                600
            } else {
                400
            })
            .color(color)
            .paint(
                Rect::new(
                    bounds.origin.x + 6.0,
                    bounds.origin.y + 4.0,
                    (bounds.size.width - 12.0).max(0.0),
                    20.0,
                ),
                context,
            );
        let rows = (((bounds.size.height - 30.0) / 19.0).max(0.0) as usize).min(3);
        for (index, title) in self.titles.iter().take(rows).enumerate() {
            let label = if index + 1 == rows && self.event_count > rows {
                format!("+{} events", self.event_count - index)
            } else {
                title.clone()
            };
            Text::caption(label)
                .color(context.theme.colors.text_secondary)
                .paint(
                    Rect::new(
                        bounds.origin.x + 6.0,
                        bounds.origin.y + 29.0 + index as f32 * 19.0,
                        (bounds.size.width - 12.0).max(0.0),
                        18.0,
                    ),
                    context,
                );
        }
        if rows == 0 && !self.titles.is_empty() {
            Text::caption(format!("{}", self.event_count))
                .color(context.theme.colors.accent)
                .paint(
                    Rect::new(
                        bounds.origin.x + bounds.size.width - 22.0,
                        bounds.origin.y + 4.0,
                        18.0,
                        20.0,
                    ),
                    context,
                );
        }
    }
}
fn event_time(event: &Event) -> String {
    event
        .time
        .map(|(a, b)| format!("{} – {}", time_label(a), time_label(b)))
        .unwrap_or_else(|| "All day".into())
}
struct AgendaViewport(CalendarView);
impl View for AgendaViewport {
    fn measure(&self, c: Constraints, _: &mut MeasureContext<'_>) -> Size {
        c.constrain(c.maximum)
    }
    fn paint(&self, bounds: Rect, context: &mut PaintContext<'_>) {
        Scroll::new(self.0.core.agenda_scroll.clone())
            .axis(ScrollAxis::Vertical)
            .content(AgendaRows {
                view: self.0.clone(),
                viewport: bounds,
            })
            .paint(bounds, context);
    }
    fn handle_event(
        &self,
        bounds: Rect,
        event: &ViewEvent,
        context: &mut EventContext<'_>,
    ) -> EventResult {
        Scroll::new(self.0.core.agenda_scroll.clone())
            .axis(ScrollAxis::Vertical)
            .content(AgendaRows {
                view: self.0.clone(),
                viewport: bounds,
            })
            .handle_event(bounds, event, context)
    }
}
struct AgendaRows {
    view: CalendarView,
    viewport: Rect,
}
impl AgendaRows {
    fn visible_range(&self, bounds: Rect, count: usize, theme: &Theme) -> std::ops::Range<usize> {
        let stride = theme.layout.list_row_height + theme.spacing.extra_small;
        let start = ((self.viewport.origin.y - bounds.origin.y) / stride)
            .floor()
            .max(0.0) as usize;
        let end = ((self.viewport.origin.y + self.viewport.size.height - bounds.origin.y) / stride)
            .ceil()
            .max(0.0) as usize;
        start.min(count)..end.saturating_add(1).min(count)
    }
    fn row(bounds: Rect, index: usize, theme: &Theme) -> Rect {
        Rect::new(
            bounds.origin.x + theme.spacing.medium,
            bounds.origin.y
                + index as f32 * (theme.layout.list_row_height + theme.spacing.extra_small),
            (bounds.size.width - theme.spacing.medium * 2.0).max(0.0),
            theme.layout.list_row_height,
        )
    }
}
impl View for AgendaRows {
    fn measure(&self, c: Constraints, context: &mut MeasureContext<'_>) -> Size {
        let model = self.view.core.model.borrow();
        let count = model
            .store
            .as_ref()
            .map(|store| store.on_date(model.selected).len())
            .unwrap_or(0);
        c.constrain(Size::new(
            c.maximum.width,
            count as f32
                * (context.theme.layout.list_row_height + context.theme.spacing.extra_small),
        ))
    }
    fn paint(&self, bounds: Rect, context: &mut PaintContext<'_>) {
        let model = self.view.core.model.borrow();
        let events = model
            .store
            .as_ref()
            .map(|store| store.on_date(model.selected))
            .unwrap_or(&[]);
        for index in self.visible_range(bounds, events.len(), context.theme) {
            let row = Self::row(bounds, index, context.theme);
            if row.intersection(self.viewport).is_some() {
                self.view
                    .row_button(&events[index], index, events.len())
                    .paint(row, context);
            }
        }
    }
    fn handle_event(
        &self,
        bounds: Rect,
        event: &ViewEvent,
        context: &mut EventContext<'_>,
    ) -> EventResult {
        let model = self.view.core.model.borrow();
        let events = model
            .store
            .as_ref()
            .map(|store| store.on_date(model.selected))
            .unwrap_or(&[]);
        let mut result = EventResult::Ignored;
        for index in self.visible_range(bounds, events.len(), context.theme()) {
            let row = Self::row(bounds, index, context.theme());
            if row.intersection(self.viewport).is_some() {
                result = result.merge(
                    self.view
                        .row_button(&events[index], index, events.len())
                        .handle_event(row, event, context),
                );
            }
        }
        result
    }
}

struct Editor {
    id: Option<u64>,
    title: TextFieldInteractionState,
    date: TextFieldInteractionState,
    start: TextFieldInteractionState,
    end: TextFieldInteractionState,
    location: TextFieldInteractionState,
    title_field: Rc<TextField>,
    date_field: Rc<TextField>,
    start_field: Rc<TextField>,
    end_field: Rc<TextField>,
    location_field: Rc<TextField>,
    all_day: State<bool>,
    all_day_control: Rc<Checkbox>,
    notes: TextEditorInteractionState,
    notes_field: Rc<TextEditor>,
    save: Rc<Button>,
    cancel: Rc<Button>,
    delete: Rc<Button>,
    confirm: Rc<Button>,
    confirm_delete: bool,
    error: Option<String>,
    scroll: ScrollState,
}
fn field(value: String, placeholder: &str) -> (TextFieldInteractionState, TextField) {
    let state = TextFieldInteractionState::new();
    state.set_value(value);
    let field = TextField::with_interaction(state.clone())
        .placeholder(placeholder)
        .size(TextFieldSize::Small);
    (state, field)
}
impl Editor {
    fn new(date: Date, event: Option<Event>, action: &Rc<Cell<Option<Action>>>) -> Self {
        let id = event.as_ref().map(|e| e.id);
        let (title, title_field) = field(
            event.as_ref().map(|e| e.title.clone()).unwrap_or_default(),
            "Event title",
        );
        let (date, date_field) = field(
            event.as_ref().map(|e| e.date).unwrap_or(date).to_string(),
            "YYYY-MM-DD",
        );
        let time = event.as_ref().and_then(|e| e.time).unwrap_or((540, 600));
        let (start, start_field) = field(time_label(time.0), "HH:MM");
        let (end, end_field) = field(time_label(time.1), "HH:MM");
        let (location, location_field) = field(
            event
                .as_ref()
                .map(|e| e.location.clone())
                .unwrap_or_default(),
            "Add a location",
        );
        let all_day = State::new(event.as_ref().is_some_and(|e| e.time.is_none()));
        let all_day_control = Checkbox::new(all_day.binding()).label("All day");
        let notes = TextEditorInteractionState::new();
        notes.set_value(event.as_ref().map(|e| e.notes.clone()).unwrap_or_default());
        let notes_field = TextEditor::with_interaction(notes.clone())
            .placeholder("Add notes")
            .line_wrap(true)
            .bordered(true);
        title.set_focused(true);
        Self {
            id,
            title,
            date,
            start,
            end,
            location,
            title_field: Rc::new(title_field),
            date_field: Rc::new(date_field),
            start_field: Rc::new(start_field),
            end_field: Rc::new(end_field),
            location_field: Rc::new(location_field),
            all_day,
            all_day_control: Rc::new(all_day_control),
            notes,
            notes_field: Rc::new(notes_field),
            save: Rc::new(button("Save", Action::Save, action).style(ButtonStyle::Accent)),
            cancel: Rc::new(button("Cancel", Action::Cancel, action)),
            delete: Rc::new(button("Delete…", Action::Delete, action).style(ButtonStyle::Ghost)),
            confirm: Rc::new(
                button("Delete Event", Action::ConfirmDelete, action).style(ButtonStyle::Danger),
            ),
            confirm_delete: false,
            error: None,
            scroll: ScrollState::new(),
        }
    }
    fn event(&self, next_id: Result<u64, String>) -> Result<Event, String> {
        let date =
            Date::parse(&self.date.value()).ok_or("Use a valid date in YYYY-MM-DD format")?;
        let time = if self.all_day.get() {
            None
        } else {
            Some((
                parse_time(&self.start.value()).ok_or("Use a start time in HH:MM format")?,
                parse_time(&self.end.value()).ok_or("Use an end time in HH:MM format")?,
            ))
        };
        let event = Event {
            id: match self.id {
                Some(id) => id,
                None => next_id?,
            },
            date,
            title: self.title.value().trim().to_owned(),
            time,
            location: self.location.value().trim().to_owned(),
            notes: self.notes.value(),
        };
        event.validate()?;
        Ok(event)
    }
    fn bounds(bounds: Rect) -> Rect {
        let width = 520.0_f32.min((bounds.size.width - 32.0).max(0.0));
        let height = 596.0_f32.min((bounds.size.height - 32.0).max(0.0));
        Rect::new(
            bounds.origin.x + (bounds.size.width - width) / 2.0,
            bounds.origin.y + (bounds.size.height - height) / 2.0,
            width,
            height,
        )
    }
    fn paint(&self, bounds: Rect, context: &mut PaintContext<'_>) {
        Rectangle::new()
            .color(RectangleColor::Custom(context.theme.shell.scrim))
            .paint(bounds, context);
        Dialog::new()
            .accessibility_label(if self.id.is_some() {
                "Edit Event"
            } else {
                "New Event"
            })
            .content(EditorContent(self))
            .paint(Self::bounds(bounds), context);
    }
    fn handle_event(
        &self,
        bounds: Rect,
        event: &ViewEvent,
        context: &mut EventContext<'_>,
    ) -> EventResult {
        Dialog::new().content(EditorContent(self)).handle_event(
            Self::bounds(bounds),
            event,
            context,
        )
    }
}
struct EditorContent<'a>(&'a Editor);
impl EditorContent<'_> {
    fn labeled(label: &str, field: &Rc<TextField>) -> VStack {
        VStack::new()
            .alignment(StackAlignment::Stretch)
            .gap(StackGap::ExtraSmall)
            .child(field.label(label))
            .child(field.clone())
    }

    fn content(&self, theme: &Theme, width: f32) -> VStack {
        let editor = self.0;
        let title = if editor.confirm_delete {
            "Delete this event?"
        } else if editor.id.is_some() {
            "Edit Event"
        } else {
            "New Event"
        };
        let body: Box<dyn View> = if editor.confirm_delete {
            Box::new(Text::body(format!(
                "“{}” will be removed from your calendar.",
                editor.title.value()
            )))
        } else {
            let mut fields = VStack::new()
                .alignment(StackAlignment::Stretch)
                .gap(StackGap::Medium)
                .child(Self::labeled("Title", &editor.title_field))
                .child(Self::labeled("Date · YYYY-MM-DD", &editor.date_field))
                .child(editor.all_day_control.clone());
            if !editor.all_day.get() {
                let start = Self::labeled("Starts · HH:MM", &editor.start_field);
                let end = Self::labeled("Ends · HH:MM", &editor.end_field);
                if width >= theme.layout.compact_form_control_width * 2.0 + theme.spacing.medium {
                    fields = fields.child(
                        HStack::new()
                            .alignment(StackAlignment::Start)
                            .gap(StackGap::Medium)
                            .child(start.layout().flex_grow(1.0))
                            .child(end.layout().flex_grow(1.0)),
                    );
                } else {
                    fields = fields.child(start).child(end);
                }
            }
            fields = fields
                .child(Self::labeled("Location", &editor.location_field))
                .child(
                    VStack::new()
                        .alignment(StackAlignment::Stretch)
                        .gap(StackGap::ExtraSmall)
                        .child(Text::label("Notes").tone(TextTone::Secondary))
                        .child(
                            editor
                                .notes_field
                                .clone()
                                .height(theme.layout.control_height * 3.0),
                        ),
                );
            Box::new(Form::new(fields))
        };
        let mut actions = HStack::new()
            .alignment(StackAlignment::Center)
            .gap(StackGap::Small);
        if editor.id.is_some() && !editor.confirm_delete {
            actions = actions.child(editor.delete.clone());
        }
        actions = actions
            .child(Spacer::new())
            .child(editor.cancel.clone())
            .child(if editor.confirm_delete {
                editor.confirm.clone()
            } else {
                editor.save.clone()
            });
        let mut content = VStack::new()
            .alignment(StackAlignment::Stretch)
            .gap(StackGap::Medium)
            .child(
                Text::styled(title, TextRole::TitleSmall)
                    .layout()
                    .flex_shrink(0.0),
            )
            .child(
                Scroll::new(editor.scroll.clone())
                    .content(body)
                    .layout()
                    .flex_grow(1.0),
            );
        if let Some(error) = &editor.error {
            content = content.child(
                Text::caption(error.clone())
                    .tone(TextTone::Destructive)
                    .layout()
                    .flex_shrink(0.0),
            );
        }
        content.child(actions.layout().flex_shrink(0.0))
    }
}
impl View for EditorContent<'_> {
    fn measure(&self, constraints: Constraints, context: &mut MeasureContext<'_>) -> Size {
        self.content(context.theme, constraints.maximum.width)
            .measure(constraints, context)
    }
    fn paint(&self, bounds: Rect, context: &mut PaintContext<'_>) {
        self.content(context.theme, bounds.size.width)
            .paint(bounds, context);
    }
    fn handle_event(
        &self,
        bounds: Rect,
        event: &ViewEvent,
        context: &mut EventContext<'_>,
    ) -> EventResult {
        let result = self
            .content(context.theme(), bounds.size.width)
            .handle_event(bounds, event, context);
        if self.0.all_day.get() {
            self.0.start.set_focused(false);
            self.0.end.set_focused(false);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use appcore::viewkit::draw_command::{DisplayList, DrawCommand};
    use appcore::viewkit::platform::{KeyModifiers, PointerButton};
    use appcore::viewkit::typography::TextMeasurer;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture {
        view: CalendarView,
        path: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "calendar-ui-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&directory).unwrap();
            let path = directory.join("events");
            Self {
                view: CalendarView::with_store(Store::load(path.clone())),
                path,
            }
        }
        fn bounds() -> Rect {
            Rect::new(0.0, 0.0, 1100.0, 730.0)
        }
        fn event(&self, event: ViewEvent) {
            let theme = Theme::current();
            let mut measurer = TextMeasurer::new();
            let mut context = EventContext::new(&theme, &theme.typography, &mut measurer);
            self.view.handle_event(Self::bounds(), &event, &mut context);
        }
        fn paint(&self) {
            let theme = Theme::current();
            let mut measurer = TextMeasurer::new();
            let mut list = DisplayList::new();
            self.view.paint(
                Self::bounds(),
                &mut PaintContext::new(&mut list, &theme, &theme.typography, &mut measurer),
            );
        }
        fn click(&self, frame: Rect) {
            self.paint();
            let position = Point::new(
                frame.origin.x + frame.size.width / 2.0,
                frame.origin.y + frame.size.height / 2.0,
            );
            self.event(ViewEvent::PointerPressed {
                position,
                button: PointerButton::Primary,
            });
            self.event(ViewEvent::PointerReleased {
                position,
                button: PointerButton::Primary,
            });
        }
        fn click_named(&self, label: &str, role: AccessibilityRole) {
            let theme = Theme::current();
            let mut list = DisplayList::new();
            let mut nodes = Vec::new();
            let mut text = TextMeasurer::new();
            self.view.paint(
                Self::bounds(),
                &mut PaintContext::new(&mut list, &theme, &theme.typography, &mut text)
                    .with_accessibility_nodes(&mut nodes),
            );
            let bounds = nodes
                .iter()
                .find(|node| node.role == role && node.label.as_deref() == Some(label))
                .expect("native control must be reachable")
                .bounds;
            self.click(bounds);
        }
        fn new_event(&self) {
            self.click_named("New Event", AccessibilityRole::Button);
        }
        fn save(&self) {
            self.click_named("Save", AccessibilityRole::Button);
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.path.parent().unwrap());
        }
    }

    fn editor_snapshot(
        editor: &Editor,
        bounds: Rect,
        theme: &Theme,
    ) -> (DisplayList, Vec<AccessibilityNode>) {
        let mut list = DisplayList::new();
        let mut nodes = Vec::new();
        let mut text = TextMeasurer::new();
        editor.paint(
            bounds,
            &mut PaintContext::new(&mut list, theme, &theme.typography, &mut text)
                .with_accessibility_nodes(&mut nodes),
        );
        (list, nodes)
    }

    #[test]
    fn editor_footer_stays_reachable_while_native_form_scrolls_in_both_themes() {
        for theme in [Theme::LIGHT, Theme::DARK] {
            for (w, h) in [(320., 300.), (520., 400.), (1100., 730.)] {
                let pending = Rc::new(Cell::new(None));
                let mut editor = Editor::new(
                    Date {
                        year: 2026,
                        month: 11,
                        day: 2,
                    },
                    None,
                    &pending,
                );
                editor.id = Some(1);
                editor.error = Some("Use a valid date in YYYY-MM-DD format".into());
                let bounds = Rect::new(0., 0., w, h);
                let (list, before) = editor_snapshot(&editor, bounds, &theme);
                let save = before
                    .iter()
                    .find(|n| n.label.as_deref() == Some("Save"))
                    .unwrap()
                    .bounds;
                let dialog = Editor::bounds(bounds);
                let times: Vec<_> = before
                    .iter()
                    .filter(|n| {
                        n.role == AccessibilityRole::TextField
                            && n.label.as_deref() == Some("HH:MM")
                    })
                    .collect();
                assert_eq!(times.len(), 2);
                if dialog.size.width - theme.dialog.padding * 2.0
                    >= theme.layout.compact_form_control_width * 2.0 + theme.spacing.medium
                {
                    assert_eq!(times[0].bounds.origin.y, times[1].bounds.origin.y);
                    assert!(
                        times[0].bounds.origin.x + times[0].bounds.size.width
                            <= times[1].bounds.origin.x
                    );
                } else {
                    assert!(
                        times[0].bounds.origin.y + times[0].bounds.size.height
                            <= times[1].bounds.origin.y
                    );
                }
                let actions: Vec<_> = before
                    .iter()
                    .filter(|n| n.role == AccessibilityRole::Button)
                    .collect();
                assert_eq!(actions.len(), 3);
                for node in &actions {
                    assert!(node.bounds.size.width > 0. && node.bounds.size.height > 0.);
                    assert!(node.bounds.origin.x >= dialog.origin.x);
                    assert!(
                        node.bounds.origin.x + node.bounds.size.width
                            <= dialog.origin.x + dialog.size.width + 0.1
                    );
                    assert!(
                        node.bounds.origin.y + node.bounds.size.height
                            <= dialog.origin.y + dialog.size.height
                    );
                }
                for pair in actions.windows(2) {
                    assert!(
                        pair[0].bounds.origin.x + pair[0].bounds.size.width
                            <= pair[1].bounds.origin.x + 0.1
                    );
                }
                let body = list
                    .commands()
                    .iter()
                    .find_map(|c| match c {
                        DrawCommand::PushClip { rect }
                            if rect.size.height > 0.
                                && rect.origin.y > dialog.origin.y
                                && rect.origin.y + rect.size.height <= save.origin.y =>
                        {
                            Some(*rect)
                        }
                        _ => None,
                    })
                    .expect("standard Scroll clips the form above the footer");
                let mut text = TextMeasurer::new();
                let mut context = EventContext::new(&theme, &theme.typography, &mut text);
                editor.handle_event(
                    bounds,
                    &ViewEvent::Scroll {
                        position: Point::new(body.origin.x + 8., body.origin.y + 8.),
                        delta_x: 0.,
                        delta_y: 1000.,
                    },
                    &mut context,
                );
                if h <= 400. {
                    assert!(editor.scroll.offset_y() > 0., "{w}x{h} must scroll");
                }
                let (_, after) = editor_snapshot(&editor, bounds, &theme);
                assert_eq!(
                    after
                        .iter()
                        .find(|n| n.label.as_deref() == Some("Save"))
                        .unwrap()
                        .bounds,
                    save
                );
                let position = Point::new(
                    save.origin.x + save.size.width / 2.,
                    save.origin.y + save.size.height / 2.,
                );
                for event in [
                    ViewEvent::PointerPressed {
                        position,
                        button: PointerButton::Primary,
                    },
                    ViewEvent::PointerReleased {
                        position,
                        button: PointerButton::Primary,
                    },
                ] {
                    editor.handle_event(bounds, &event, &mut context);
                }
                assert!(pending.get() == Some(Action::Save));
            }
        }
    }

    #[test]
    fn all_day_hides_time_controls_without_discarding_their_values() {
        let pending = Rc::new(Cell::new(None));
        let editor = Editor::new(
            Date {
                year: 2026,
                month: 11,
                day: 2,
            },
            None,
            &pending,
        );
        let bounds = Fixture::bounds();
        let theme = Theme::LIGHT;
        editor.start.set_value("12:30");
        editor.end.set_value("13:45");
        let (_, nodes) = editor_snapshot(&editor, bounds, &theme);
        let checkbox = nodes
            .iter()
            .find(|n| n.role == AccessibilityRole::Checkbox)
            .unwrap()
            .bounds;
        let position = Point::new(
            checkbox.origin.x + 8.,
            checkbox.origin.y + checkbox.size.height / 2.,
        );
        let mut text = TextMeasurer::new();
        let mut context = EventContext::new(&theme, &theme.typography, &mut text);
        for event in [
            ViewEvent::PointerPressed {
                position,
                button: PointerButton::Primary,
            },
            ViewEvent::PointerReleased {
                position,
                button: PointerButton::Primary,
            },
        ] {
            editor.handle_event(bounds, &event, &mut context);
        }
        assert!(editor.all_day.get());
        let (_, nodes) = editor_snapshot(&editor, bounds, &theme);
        assert!(
            !nodes
                .iter()
                .any(|n| n.role == AccessibilityRole::TextField
                    && n.label.as_deref() == Some("HH:MM"))
        );
        assert_eq!(editor.start.value(), "12:30");
        assert_eq!(editor.end.value(), "13:45");
        editor.all_day.set(false);
        let (_, nodes) = editor_snapshot(&editor, bounds, &theme);
        assert_eq!(
            nodes
                .iter()
                .filter(|n| n.role == AccessibilityRole::TextField
                    && n.label.as_deref() == Some("HH:MM"))
                .count(),
            2
        );
    }

    #[test]
    fn native_controls_create_edit_and_confirm_delete_a_persisted_event() {
        let f = Fixture::new();
        f.new_event();
        f.event(ViewEvent::TextInput {
            text: "打ち合わせ".into(),
        });
        f.save();
        assert!(f.view.core.editor.borrow().is_none());
        let events = Store::load(f.path.clone()).unwrap().events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "打ち合わせ");
        f.click_named("打ち合わせ", AccessibilityRole::ListItem);
        f.view
            .core
            .editor
            .borrow()
            .as_ref()
            .unwrap()
            .title
            .set_value("Changed");
        f.save();
        assert_eq!(
            Store::load(f.path.clone()).unwrap().events[0].title,
            "Changed"
        );
        f.click_named("Changed", AccessibilityRole::ListItem);
        f.click_named("Delete…", AccessibilityRole::Button);
        assert!(f.view.core.editor.borrow().as_ref().unwrap().confirm_delete);
        assert_eq!(Store::load(f.path.clone()).unwrap().events.len(), 1);
        f.event(ViewEvent::KeyPressed {
            key: Key::Escape,
            modifiers: KeyModifiers::default(),
        });
        assert!(!f.view.core.editor.borrow().as_ref().unwrap().confirm_delete);
        assert_eq!(
            f.view.core.editor.borrow().as_ref().unwrap().title.value(),
            "Changed"
        );
        f.click_named("Delete…", AccessibilityRole::Button);
        f.click_named("Delete Event", AccessibilityRole::Button);
        assert!(Store::load(f.path.clone()).unwrap().events.is_empty());
    }
    #[test]
    fn invalid_and_failed_saves_keep_the_editor_and_input() {
        let f = Fixture::new();
        f.new_event();
        {
            let editor = f.view.core.editor.borrow();
            let editor = editor.as_ref().unwrap();
            editor.title.set_value("Keep my edit");
            editor.start.set_value("12:00");
            editor.end.set_value("11:00");
        }
        f.save();
        assert!(
            f.view
                .core
                .editor
                .borrow()
                .as_ref()
                .unwrap()
                .error
                .is_some()
        );
        assert!(Store::load(f.path.clone()).unwrap().events.is_empty());
        f.view
            .core
            .editor
            .borrow()
            .as_ref()
            .unwrap()
            .end
            .set_value("13:00");
        std::fs::create_dir(f.path.with_extension("lock")).unwrap();
        f.save();
        assert_eq!(
            f.view.core.editor.borrow().as_ref().unwrap().title.value(),
            "Keep my edit"
        );
        assert!(
            f.view
                .core
                .editor
                .borrow()
                .as_ref()
                .unwrap()
                .error
                .as_ref()
                .unwrap()
                .contains("busy")
        );
        std::fs::remove_dir(f.path.with_extension("lock")).unwrap();
        f.save();
        assert_eq!(Store::load(f.path.clone()).unwrap().events.len(), 1);
    }
    #[test]
    fn escape_discards_draft_and_arrow_navigation_crosses_months() {
        let f = Fixture::new();
        f.new_event();
        f.event(ViewEvent::TextInput {
            text: "Unsaved".into(),
        });
        f.event(ViewEvent::KeyPressed {
            key: Key::Escape,
            modifiers: KeyModifiers::default(),
        });
        assert!(f.view.core.editor.borrow().is_none());
        assert!(Store::load(f.path.clone()).unwrap().events.is_empty());
        f.view
            .core
            .action
            .set(Some(Action::Select(Date::parse("2026-12-31").unwrap())));
        f.view.apply_action();
        f.event(ViewEvent::KeyPressed {
            key: Key::ArrowRight,
            modifiers: KeyModifiers::default(),
        });
        assert_eq!(
            f.view.core.model.borrow().selected.to_string(),
            "2027-01-01"
        );
        assert_eq!(f.view.core.model.borrow().month.to_string(), "2027-01-01");
    }
    #[test]
    fn layout_stays_within_wide_and_stacked_windows_and_icon_decodes() {
        let theme = Theme::current();
        for (w, h) in [(1100.0, 730.0), (760.0, 600.0), (640.0, 600.0)] {
            let bounds = Rect::new(0.0, 0.0, w, h);
            let frames = Frames::new(bounds, &theme);
            for rect in [
                frames.month,
                frames.agenda,
                frames.grid_cell(0),
                frames.grid_cell(41),
            ] {
                assert!(rect.origin.x >= 0.0 && rect.origin.y >= 0.0);
                assert!(rect.origin.x + rect.size.width <= w + 0.01);
                assert!(rect.origin.y + rect.size.height <= h + 0.01);
            }
        }
        assert!(SvgData::decode(include_bytes!("../appicon.svg")).is_ok());
    }

    fn snapshot(
        view: &CalendarView,
        width: f32,
        theme: &Theme,
    ) -> (
        DisplayList,
        Vec<appcore::viewkit::accessibility::AccessibilityNode>,
    ) {
        let mut list = DisplayList::new();
        let mut nodes = Vec::new();
        let mut text = TextMeasurer::new();
        let bounds = Rect::new(0.0, 0.0, width, 730.0);
        view.paint(
            bounds,
            &mut PaintContext::new(&mut list, theme, &theme.typography, &mut text)
                .with_accessibility_nodes(&mut nodes),
        );
        (list, nodes)
    }

    #[test]
    fn native_toolbar_and_month_pane_fit_wide_and_stacked_layouts_in_both_themes() {
        let fixture = Fixture::new();
        for theme in [Theme::LIGHT, Theme::DARK] {
            for width in [320.0, 640.0, 760.0, 1100.0] {
                let (list, nodes) = snapshot(&fixture.view, width, &theme);
                assert!(nodes.iter().any(|n| n.role == AccessibilityRole::Toolbar));
                assert!(
                    nodes
                        .iter()
                        .any(|n| n.label.as_deref() == Some("Previous month"))
                );
                assert!(
                    nodes
                        .iter()
                        .any(|n| matches!(n.label.as_deref(), Some("New Event" | "More actions")))
                );
                for node in nodes.iter().filter(|n| {
                    n.role == AccessibilityRole::Button
                        && n.bounds.origin.y < theme.layout.top_bar_height
                }) {
                    assert!(node.bounds.origin.x >= 0.0);
                    assert!(node.bounds.origin.x + node.bounds.size.width <= width + 0.01);
                }
                let month = Frames::new(Rect::new(0.0, 0.0, width, 730.0), &theme).month;
                let clip =
                    list.commands()
                        .iter()
                        .enumerate()
                        .find_map(|(i, command)| match command {
                            DrawCommand::PushRoundedCornersClip { rect, radii }
                                if *rect == month =>
                            {
                                Some((i, *radii))
                            }
                            _ => None,
                        });
                if width >= 760.0 {
                    let (index, radii) =
                        clip.expect("the month/agenda chrome junction must be rounded");
                    assert_eq!(radii, [0.0, theme.radius.extra_large, 0.0, 0.0]);
                    let corner =
                        Point::new(month.origin.x + month.size.width - 0.5, month.origin.y);
                    let backdrop =
                        list.commands()[..index]
                            .iter()
                            .rev()
                            .find_map(|command| match command {
                                DrawCommand::FillRect { rect, color } if rect.contains(corner) => {
                                    Some(*color)
                                }
                                _ => None,
                            });
                    assert_eq!(backdrop, Some(theme.surface.sidebar_background));
                } else {
                    assert!(clip.is_none(), "a full-width month is not a floating card");
                }
            }
        }
    }

    #[test]
    fn native_agenda_scroll_keeps_rows_bounded_and_opens_the_visible_event() {
        let fixture = Fixture::new();
        let selected = fixture.view.core.model.borrow().selected;
        let events = (1..=1000)
            .map(|id| Event {
                id,
                date: selected,
                title: format!("Event {id:04}"),
                time: None,
                location: String::new(),
                notes: String::new(),
            })
            .collect();
        fixture
            .view
            .core
            .model
            .borrow_mut()
            .store
            .as_mut()
            .unwrap()
            .save(events)
            .unwrap();
        let theme = Theme::LIGHT;
        let stride = theme.layout.list_row_height + theme.spacing.extra_small;
        fixture
            .view
            .core
            .agenda_scroll
            .set_offset(0.0, stride * 100.0);
        let (_, nodes) = snapshot(&fixture.view, 1100.0, &theme);
        let rows: Vec<_> = nodes
            .iter()
            .filter(|n| n.role == AccessibilityRole::ListItem)
            .collect();
        assert!(!rows.is_empty() && rows.len() <= 12);
        assert_eq!(rows[0].label.as_deref(), Some("Event 0101"));
        fixture.click_named("Event 0101", AccessibilityRole::ListItem);
        assert_eq!(
            fixture.view.core.editor.borrow().as_ref().unwrap().id,
            Some(101)
        );
        fixture.view.core.action.set(Some(Action::Cancel));
        fixture.view.apply_action();
        fixture
            .view
            .core
            .action
            .set(Some(Action::Select(selected.add_days(1).unwrap())));
        fixture.view.apply_action();
        assert_eq!(fixture.view.core.agenda_scroll.offset_y(), 0.0);
        assert_eq!(
            Store::load(fixture.path.clone()).unwrap().events.len(),
            1000
        );
    }

    #[test]
    fn day_selection_uses_shared_active_and_inactive_selection_colors() {
        for theme in [Theme::LIGHT, Theme::DARK] {
            for active in [true, false] {
                let mut list = DisplayList::new();
                let mut text = TextMeasurer::new();
                DayCell {
                    date: Date::new(2026, 11, 1).unwrap(),
                    outside: false,
                    selected: true,
                    today: true,
                    titles: Vec::new(),
                    event_count: 0,
                }
                .paint(
                    Rect::new(0.0, 0.0, 80.0, 80.0),
                    &mut PaintContext::new(&mut list, &theme, &theme.typography, &mut text)
                        .with_window_active(active),
                );
                let expected = if active {
                    theme.list.selected_background
                } else {
                    theme.list.inactive_selected_background
                };
                assert!(list.commands().iter().any(|command| matches!(command,
                    DrawCommand::FillRoundedRect { color, .. } if *color == expected
                )));
            }
        }
    }
}
