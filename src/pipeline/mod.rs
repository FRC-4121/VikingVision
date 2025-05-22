pub mod component;
pub mod daemon;
pub mod runner;

pub mod prelude {
    pub use super::component::{Component, ComponentFactory, Data, Inputs, OutputKind};
    pub use super::runner::{ComponentContext, PipelineRunner, RunParams};
    pub use crate::utils::LogErr;

    #[doc(hidden)]
    use std::sync::Arc;

    #[doc(hidden)]
    pub struct ProduceComponent;
    impl ProduceComponent {
        pub fn new() -> Self {
            Self
        }
    }
    impl Component for ProduceComponent {
        fn inputs(&self) -> Inputs {
            Inputs::none()
        }
        fn output_kind(&self, name: Option<&str>) -> OutputKind {
            if name.is_none() {
                OutputKind::Single
            } else {
                OutputKind::None
            }
        }
        fn run<'s, 'r: 's>(&self, ctx: ComponentContext<'r, '_, 's>) {
            ctx.submit(None, Arc::new("data".to_string()));
        }
    }

    #[doc(hidden)]
    pub struct ConsumeComponent;
    impl Component for ConsumeComponent {
        fn inputs(&self) -> Inputs {
            Inputs::Primary
        }
        fn output_kind(&self, _: Option<&str>) -> OutputKind {
            OutputKind::None
        }
        fn run<'s, 'r: 's>(&self, _: ComponentContext<'r, '_, 's>) {}
    }

    #[doc(hidden)]
    pub fn produce_component() -> Arc<dyn Component> {
        Arc::new(ProduceComponent)
    }

    #[doc(hidden)]
    pub fn consume_component() -> Arc<dyn Component> {
        Arc::new(ConsumeComponent)
    }
}
