use std::borrow::Cow;
use tracing::{Subscriber, span};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

pub const COMPONENT_RUN_TARGET: &str = "viking_vision::pipeline::runner::special_run_span";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EventPos {
    index: u64,
    line: u64,
    file: Cow<'static, str>,
}

/// A [`Layer`] that filters duplicate events coming from components.
///
/// Identical events are events that:
/// - originated in the same component (determined by the span that they're run in)
/// - have the same source location (file and line)
///
/// Source locations can be overridden with the `source.file` and `source.line` fields, which
/// allows caller tracking.
#[derive(Debug, Default, Clone)]
pub struct ComponentEventFilter {
    seen: papaya::HashSet<EventPos>,
    enabled: bool,
}
impl ComponentEventFilter {
    pub fn new(enabled: bool) -> Self {
        Self {
            seen: papaya::HashSet::new(),
            enabled,
        }
    }
    /// Clear all events from this layer.
    pub fn clear_seen(&self) {
        self.seen.pin().clear();
    }
    /// Control whether this layer is enabled or disabled.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}
impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for ComponentEventFilter {
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        if !self.enabled {
            return;
        }
        if attrs.metadata().target() != COMPONENT_RUN_TARGET {
            return;
        }
        let mut visitor = SpanVisitor(None);
        attrs.record(&mut visitor);
        if let Some(index) = visitor.0 {
            let span = ctx.span(id).expect("Missing span!");
            span.extensions_mut().insert(IndexStorage(index));
        }
    }
    fn event_enabled(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) -> bool {
        if !self.enabled {
            return true;
        }
        let mut visitor = EventVisitor {
            noisy: false,
            file: None,
            line: None,
        };
        event.record(&mut visitor);
        if visitor.noisy {
            true
        } else if let Some(index) = ctx
            .event_scope(event)
            .and_then(|mut i| i.find_map(|s| s.extensions().get::<IndexStorage>().map(|s| s.0)))
        {
            let meta = event.metadata();
            let file = visitor.file.map_or(
                Cow::Borrowed(meta.file().unwrap_or("<unknown>")),
                Cow::Owned,
            );
            let line = visitor.line.or(meta.line().map(From::from)).unwrap_or(0);
            self.seen.pin().insert(EventPos { index, line, file })
        } else {
            true
        }
    }
}

struct IndexStorage(u64);

struct SpanVisitor(Option<u64>);
impl tracing::field::Visit for SpanVisitor {
    fn record_debug(&mut self, _field: &tracing::field::Field, _value: &dyn core::fmt::Debug) {}
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        if field.name() == "_index" {
            self.0 = Some(value);
        }
    }
}
struct EventVisitor {
    noisy: bool,
    file: Option<String>,
    line: Option<u64>,
}
impl tracing::field::Visit for EventVisitor {
    fn record_debug(&mut self, _field: &tracing::field::Field, _value: &dyn core::fmt::Debug) {}
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        if field.name() == "allow_noisy" {
            self.noisy = value;
        }
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if !self.noisy && field.name() == "source.file" {
            // avoid allocating if we're just going to ignore it
            self.file = Some(value.to_string());
        }
    }
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        if field.name() == "source.line" {
            self.line = Some(value);
        }
    }
}
