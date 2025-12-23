#![deny(missing_docs)]

//! Agent Inbox Protocol: A streaming combinator library for LLM agent development.
//!
//! This crate provides high-order functions to build LLM agents using a streaming architecture.
//! The fundamental unit is `StreamingResponse`, which represents a single LLM turn (a finite
//! stream of tokens). Agents are built by composing these streams using combinators.
//!
//! # Architecture Layers
//!
//! 1. **Inner Layer (Token Stream):** Use `filter`, `passthrough`, `collect`.
//! 2. **Middle Layer (Turn):** Use `fold` to turn a Stream into State.
//! 3. **Outer Layer (Agent Stream):** Use `unfold` to turn State into an infinite series of Turns.

use std::convert::Infallible;
use std::future::Future;
use std::io::Write;
use std::pin::Pin;
use std::sync::Arc;

use futures::Stream;
use futures::StreamExt;

use crate::{
    Anthropic, ContentBlock, ContentBlockDelta, Error, Message, MessageCreateParams,
    MessageCreateTemplate, MessageParam, MessageStreamEvent, StopReason, ToolResultBlock,
    ToolResultBlockContent, ToolUseBlock,
};

////////////////////////////////////////// read_user_input //////////////////////////////////////////

/// Reads user input from stdin with a "You: " prompt.
///
/// Prompts the user with "You: " and reads a line of input from stdin. The input is trimmed
/// of leading and trailing whitespace.
///
/// Returns `Some(input)` with the trimmed input on success, or `None` when the user wants
/// to quit or input cannot be read.
///
/// # Returns
///
/// * `Some(String)` - The trimmed user input
/// * `None` - When any of the following occurs:
///   - EOF (Ctrl+D)
///   - Empty input (blank line)
///   - "quit" or "exit" commands (case-insensitive)
///   - Read errors
///
/// # Example
///
/// This function is typically used in an agent loop:
///
/// ```no_run
/// use claudius::combinators::read_user_input;
///
/// loop {
///     let input = match read_user_input() {
///         Some(input) => input,
///         None => break, // User wants to quit
///     };
///     println!("You said: {}", input);
/// }
/// ```
pub fn read_user_input() -> Option<String> {
    print!("\nYou: ");
    std::io::stdout().flush().ok()?;

    let mut input = String::new();
    match std::io::stdin().read_line(&mut input) {
        Ok(0) => None,
        Ok(_) => {
            let trimmed = input.trim();
            if trimmed.is_empty()
                || trimmed.eq_ignore_ascii_case("quit")
                || trimmed.eq_ignore_ascii_case("exit")
            {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Err(_) => None,
    }
}

////////////////////////////////////////////// Context /////////////////////////////////////////////

/// A trait for types that can be prepared into a sequence of messages for an LLM API call.
///
/// The `Context` trait abstracts over different ways of managing conversation state.
/// Implementors define how to convert their internal state into `Vec<MessageParam>` for API calls,
/// and optionally support injection of external threads.
///
/// Most implementations should use the [`impl_simple_context!`] macro, which generates a
/// standard implementation that delegates to an inner [`VecContext`] field.
///
/// # Examples
///
/// Using the macro for simple cases:
///
/// ```
/// # use claudius::combinators::{VecContext, Context};
/// # use claudius::impl_simple_context;
///
/// #[derive(Clone)]
/// struct MyState {
///     thread: VecContext,
///     custom_field: String,
/// }
///
/// impl_simple_context!(MyState, thread);
/// ```
///
/// Manual implementation for advanced use cases:
///
/// ```
/// # use claudius::combinators::{VecContext, Context};
/// # use claudius::{MessageParam, Error, push_or_merge_message};
/// use std::convert::Infallible;
///
/// #[derive(Clone)]
/// struct PrefixedContext {
///     prefix: String,
///     messages: Vec<MessageParam>,
/// }
///
/// impl Context for PrefixedContext {
///     type InjectionControl = Infallible;
///
///     #[allow(unreachable_code)]
///     fn inject(
///         self,
///         control: Self::InjectionControl,
///         _thread: Option<Vec<MessageParam>>,
///     ) -> Result<impl Context, Error> {
///         match control {}
///         Ok(self)
///     }
///
///     fn prepare(self) -> Vec<MessageParam> {
///         let mut result = vec![MessageParam::user(&self.prefix)];
///         result.extend(self.messages);
///         result
///     }
///
///     fn push_or_merge_message(&mut self, message: MessageParam) {
///         push_or_merge_message(&mut self.messages, message);
///     }
/// }
/// ```
pub trait Context {
    /// The type used to control injection behavior.
    ///
    /// Use [`std::convert::Infallible`] if injection is not supported. The injection control
    /// allows callers to specify how external threads should be merged into the context.
    type InjectionControl;

    /// Injects an external thread into this context.
    ///
    /// For contexts that don't support injection, use `Infallible` as `InjectionControl`
    /// and the [`impl_simple_context!`] macro to generate the implementation.
    fn inject(
        self,
        control: Self::InjectionControl,
        thread: Option<Vec<MessageParam>>,
    ) -> Result<impl Context, Error>;

    /// Converts this context into a sequence of messages ready for an API call.
    ///
    /// This method consumes the context and produces the final message list that will be
    /// sent to the LLM API.
    fn prepare(self) -> Vec<MessageParam>;

    /// Pushes a message into this context, merging with the last message if they share the
    /// same role.
    ///
    /// This method allows appending messages to a context without requiring `From<VecContext>`,
    /// preserving any additional state the context may hold (such as database connections).
    fn push_or_merge_message(&mut self, message: MessageParam);
}

//////////////////////////////////////// impl_simple_context! ///////////////////////////////////////

/// Implements the [`Context`] trait for a type that delegates to a [`VecContext`] field.
///
/// This macro eliminates boilerplate for state types that wrap a `VecContext` and don't
/// support injection. The generated implementation uses [`std::convert::Infallible`] as
/// the injection control type, making injection effectively impossible.
///
/// # Example
///
/// ```
/// # use claudius::combinators::{VecContext, Context};
/// # use claudius::impl_simple_context;
///
/// #[derive(Clone)]
/// struct ChatState {
///     thread: VecContext,
///     should_quit: bool,
/// }
///
/// impl_simple_context!(ChatState, thread);
///
/// // Now ChatState can be used as a Context
/// let state = ChatState {
///     thread: VecContext(vec![]),
///     should_quit: false,
/// };
/// let messages = state.prepare();
/// assert!(messages.is_empty());
/// ```
#[macro_export]
macro_rules! impl_simple_context {
    ($type:ty, $thread_field:ident) => {
        impl $crate::combinators::Context for $type {
            type InjectionControl = std::convert::Infallible;

            #[allow(unreachable_code)]
            fn inject(
                self,
                control: Self::InjectionControl,
                _thread: Option<Vec<$crate::MessageParam>>,
            ) -> Result<impl $crate::combinators::Context, $crate::Error> {
                match control {};
                Ok(self)
            }

            fn prepare(self) -> Vec<$crate::MessageParam> {
                self.$thread_field.prepare()
            }

            fn push_or_merge_message(&mut self, message: $crate::MessageParam) {
                self.$thread_field.push_or_merge_message(message);
            }
        }
    };
}

/////////////////////////////////////// impl_from_vec_context! //////////////////////////////////////

/// Implements `From<VecContext>` for a state type, initializing other fields with defaults.
///
/// This macro generates a `From<VecContext>` implementation that sets the `thread` field
/// from the input and initializes all other specified fields with the provided default values.
/// This is useful when constructing state from a `VecContext` in examples or tests.
///
/// # Example
///
/// ```
/// # use claudius::combinators::VecContext;
/// # use claudius::{impl_from_vec_context, MessageParam};
///
/// #[derive(Clone)]
/// struct ChatState {
///     thread: VecContext,
///     should_quit: bool,
///     turn_count: usize,
/// }
///
/// impl_from_vec_context!(ChatState { should_quit: false, turn_count: 0 });
///
/// // Converting from VecContext initializes other fields to defaults
/// let messages = vec![MessageParam::user("Hello!")];
/// let state = ChatState::from(VecContext(messages));
/// assert!(!state.should_quit);
/// assert_eq!(state.turn_count, 0);
/// ```
#[macro_export]
macro_rules! impl_from_vec_context {
    ($type:ty { $($field:ident: $default:expr),* $(,)? }) => {
        impl From<$crate::combinators::VecContext> for $type {
            fn from(ctx: $crate::combinators::VecContext) -> Self {
                Self {
                    thread: ctx,
                    $($field: $default,)*
                }
            }
        }
    };
}

/////////////////////////////////////////////// tool! ///////////////////////////////////////////////

/// Creates a `crate::ToolUnionParam::CustomTool` with less boilerplate.
///
/// This macro simplifies tool definition by providing a concise syntax for creating
/// tool parameters with JSON schemas. The macro expands to construct a [`crate::ToolParam`]
/// wrapped in [`crate::ToolUnionParam::CustomTool`].
///
/// # Arguments
///
/// * `$name` - The tool name as a string literal
/// * `$description` - A human-readable description of what the tool does
/// * `$schema` - A JSON object literal defining the input schema (JSON Schema format)
///
/// # Example
///
/// ```
/// # use claudius::tool;
///
/// let tools = vec![
///     tool!("get_weather", "Get the current weather for a location", {
///         "type": "object",
///         "properties": {
///             "location": {
///                 "type": "string",
///                 "description": "The city and state, e.g. San Francisco, CA"
///             }
///         },
///         "required": ["location"]
///     }),
///     tool!("calculator", "Perform basic arithmetic", {
///         "type": "object",
///         "properties": {
///             "operation": { "type": "string", "enum": ["add", "subtract", "multiply", "divide"] },
///             "a": { "type": "number" },
///             "b": { "type": "number" }
///         },
///         "required": ["operation", "a", "b"]
///     }),
/// ];
///
/// assert_eq!(tools.len(), 2);
/// ```
#[macro_export]
macro_rules! tool {
    ($name:expr, $description:expr, $schema:tt) => {
        $crate::ToolUnionParam::CustomTool($crate::ToolParam {
            name: $name.to_string(),
            description: Some($description.to_string()),
            input_schema: serde_json::json!($schema),
            cache_control: None,
        })
    };
}

//////////////////////////////////////////// VecContext ////////////////////////////////////////////

/// A simple conversation context backed by a vector of messages.
///
/// `VecContext` is the most basic implementation of the [`Context`] trait, wrapping a
/// `Vec<MessageParam>` to hold conversation history. It serves as both a standalone context
/// type and as the canonical storage format for more complex context implementations.
///
/// # Examples
///
/// Creating a context from scratch:
///
/// ```
/// # use claudius::combinators::VecContext;
/// # use claudius::MessageParam;
///
/// let messages = vec![
///     MessageParam::user("Hello, Claude!"),
///     MessageParam::assistant("Hello! How can I help you today?"),
/// ];
/// let ctx = VecContext(messages);
/// ```
///
/// Using `VecContext` as a baseline for custom state types via [`impl_simple_context!`]:
///
/// ```
/// # use claudius::combinators::{VecContext, Context};
/// # use claudius::impl_simple_context;
///
/// #[derive(Clone)]
/// struct ChatState {
///     thread: VecContext,
///     turn_count: usize,
/// }
///
/// impl_simple_context!(ChatState, thread);
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VecContext(pub Vec<MessageParam>);

impl Context for VecContext {
    type InjectionControl = Infallible;

    #[allow(unreachable_code)]
    fn inject(
        self,
        control: Self::InjectionControl,
        _thread: Option<Vec<MessageParam>>,
    ) -> Result<impl Context, Error> {
        match control {};
        Ok(self)
    }

    fn prepare(self) -> Vec<MessageParam> {
        self.0
    }

    fn push_or_merge_message(&mut self, message: MessageParam) {
        crate::push_or_merge_message(&mut self.0, message);
    }
}

/////////////////////////////////////////// Tuple-Context //////////////////////////////////////////

macro_rules! impl_tuple_context {
    ($last:ident ; 0) => {
        #[allow(non_snake_case)]
        impl<$last: Context> Context for ($last,) {
            type InjectionControl = Infallible;

            #[allow(unreachable_code)]
            fn inject(self, control: Self::InjectionControl, _thread: Option<Vec<MessageParam>>) -> Result<impl Context, Error> {
                match control {};
                Ok(self)
            }

            fn prepare(self) -> Vec<MessageParam> {
                let mut result = vec![];
                let ($last,) = self;
                let prepped = $last.prepare();
                for mp in prepped.into_iter() {
                    crate::push_or_merge_message(&mut result, mp);
                }
                result
            }

            fn push_or_merge_message(&mut self, message: MessageParam) {
                self.0.push_or_merge_message(message);
            }
        }
    };
    ($($name:ident)+ ; $last:ident ; $last_idx:tt) => {
        #[allow(non_snake_case)]
        impl<$($name: Context,)+ $last: Context> Context for ($($name,)+ $last,) {
            type InjectionControl = Infallible;

            #[allow(unreachable_code)]
            fn inject(self, control: Self::InjectionControl, _thread: Option<Vec<MessageParam>>) -> Result<impl Context, Error> {
                match control {};
                Ok(self)
            }

            fn prepare(self) -> Vec<MessageParam> {
                let mut result = vec![];
                let ($($name,)+ $last,) = self;
                $(
                    let prepped = $name.prepare();
                    for mp in prepped.into_iter() {
                        crate::push_or_merge_message(&mut result, mp);
                    }
                )+
                let prepped = $last.prepare();
                for mp in prepped.into_iter() {
                    crate::push_or_merge_message(&mut result, mp);
                }
                result
            }

            fn push_or_merge_message(&mut self, message: MessageParam) {
                self.$last_idx.push_or_merge_message(message);
            }
        }
    };
}

impl_tuple_context! { A ; 0 }
impl_tuple_context! { A ; B ; 1 }
impl_tuple_context! { A B ; C ; 2 }
impl_tuple_context! { A B C ; D ; 3 }
impl_tuple_context! { A B C D ; E ; 4 }
impl_tuple_context! { A B C D E ; F ; 5 }
impl_tuple_context! { A B C D E F ; G ; 6 }
impl_tuple_context! { A B C D E F G ; H ; 7 }
impl_tuple_context! { A B C D E F G H ; I ; 8 }
impl_tuple_context! { A B C D E F G H I ; J ; 9 }
impl_tuple_context! { A B C D E F G H I J ; K ; 10 }
impl_tuple_context! { A B C D E F G H I J K ; L ; 11 }
impl_tuple_context! { A B C D E F G H I J K L ; M ; 12 }
impl_tuple_context! { A B C D E F G H I J K L M ; N ; 13 }
impl_tuple_context! { A B C D E F G H I J K L M N ; O ; 14 }
impl_tuple_context! { A B C D E F G H I J K L M N O ; P ; 15 }
impl_tuple_context! { A B C D E F G H I J K L M N O P ; Q ; 16 }
impl_tuple_context! { A B C D E F G H I J K L M N O P Q ; R ; 17 }
impl_tuple_context! { A B C D E F G H I J K L M N O P Q R ; S ; 18 }
impl_tuple_context! { A B C D E F G H I J K L M N O P Q R S ; T ; 19 }
impl_tuple_context! { A B C D E F G H I J K L M N O P Q R S T ; U ; 20 }
impl_tuple_context! { A B C D E F G H I J K L M N O P Q R S T U ; V ; 21 }
impl_tuple_context! { A B C D E F G H I J K L M N O P Q R S T U V ; W ; 22 }
impl_tuple_context! { A B C D E F G H I J K L M N O P Q R S T U V W ; X ; 23 }
impl_tuple_context! { A B C D E F G H I J K L M N O P Q R S T U V W X ; Y ; 24 }
impl_tuple_context! { A B C D E F G H I J K L M N O P Q R S T U V W X Y ; Z ; 25 }

////////////////////////////////////////////// client //////////////////////////////////////////////

/// Creates an Anthropic client function that generates streaming responses.
///
/// This is the entry point for making LLM calls. The returned function takes
/// `MessageCreateTemplate` and returns a stream of `Result<MessageStreamEvent, Error>`.
#[allow(clippy::type_complexity)]
pub fn client<C: Context + Clone + Send + 'static>(
    api_key: Option<String>,
) -> impl Fn(
    MessageCreateTemplate,
    C,
) -> Pin<
    Box<
        dyn Future<
                Output = Result<
                    Pin<Box<dyn Stream<Item = Result<MessageStreamEvent, Error>> + Send>>,
                    Error,
                >,
            > + Send,
    >,
> + Clone {
    let anthropic = Anthropic::new(api_key);
    move |template: MessageCreateTemplate, ctx: C| {
        let anthropic = anthropic.clone();
        Box::pin(async move {
            let anthropic = anthropic?;
            let mut mcp = MessageCreateParams::default();
            mcp = template.apply(mcp);
            mcp.messages = ctx.prepare();
            match anthropic.stream(&mcp).await {
                Ok(stream) => Ok(Box::pin(stream) as _),
                Err(err) => Ok(Box::pin(futures::stream::iter(vec![Err(err)])) as _),
            }
        })
    }
}

/////////////////////////////////////////// filter_map /////////////////////////////////////////////

/// Filters and transforms stream events in one pass.
///
/// The predicate returns `Some(T)` to include a transformed event, or `None` to skip it.
#[allow(clippy::type_complexity)]
pub fn filter_map<'a, T, U, P>(
    predicate: P,
) -> impl Fn(Pin<Box<dyn Stream<Item = T>>>) -> Pin<Box<dyn Stream<Item = U> + 'a>> + Clone
where
    T: 'a,
    P: Fn(T) -> Option<U> + Send + 'a,
{
    let predicate = Arc::new(predicate);
    move |stream| {
        let predicate = Arc::clone(&predicate);
        Box::pin(stream.filter_map(move |t| {
            let predicate = Arc::clone(&predicate);
            async move { predicate(t) }
        }))
    }
}

/// Async version of `filter_map` that allows async transformation.
#[allow(clippy::type_complexity)]
pub fn filter_map_async<'a, T, U, P>(
    predicate: P,
) -> impl Fn(Pin<Box<dyn Stream<Item = T>>>) -> Pin<Box<dyn Stream<Item = U> + 'a>> + Clone
where
    T: 'a,
    U: 'a,
    P: Fn(T) -> Pin<Box<dyn Future<Output = Option<U>> + Send + 'a>> + Send + 'a,
{
    let predicate = Arc::new(predicate);
    move |stream| {
        let predicate = Arc::clone(&predicate);
        Box::pin(stream.filter_map(move |t| {
            let predicate = Arc::clone(&predicate);
            predicate(t)
        }))
    }
}

////////////////////////////////////////////// filter //////////////////////////////////////////////

/// Filters stream events based on a predicate.
///
/// Only events for which the predicate returns `true` are passed through.
#[allow(clippy::type_complexity)]
pub fn filter<'a, T, P>(
    predicate: P,
) -> impl Fn(Pin<Box<dyn Stream<Item = T>>>) -> Pin<Box<dyn Stream<Item = T> + 'a>> + Clone
where
    T: 'a,
    P: for<'b> Fn(&'b T) -> bool + Send + 'a,
{
    filter_map(move |t| if (predicate)(&t) { Some(t) } else { None })
}

/// Async version of `filter` that allows async predicates.
#[allow(clippy::type_complexity)]
pub fn filter_async<'a, T, P>(
    predicate: P,
) -> impl Fn(Pin<Box<dyn Stream<Item = T>>>) -> Pin<Box<dyn Stream<Item = T> + 'a>> + Clone
where
    T: 'a,
    P: for<'b> Fn(&'b T) -> Pin<Box<dyn Future<Output = bool> + Send + 'b>> + Send + 'a,
{
    let predicate = Arc::new(predicate);
    move |stream| {
        let predicate = Arc::clone(&predicate);
        Box::pin(stream.filter_map(move |t| {
            let predicate = Arc::clone(&predicate);
            Box::pin(async move { if predicate(&t).await { Some(t) } else { None } })
        }))
    }
}

/////////////////////////////////////////////// fold ///////////////////////////////////////////////

/// Folds a stream into a single accumulated value using a default initial value.
///
/// This is a curried version that returns an async function suitable for chaining.
/// The accumulator type must implement `Default`.
#[allow(clippy::type_complexity)]
pub async fn fold<'a, A, T, F>(
    f: F,
) -> impl Fn(Pin<Box<dyn Stream<Item = T>>>) -> Pin<Box<dyn Future<Output = A> + 'a>> + Clone
where
    A: Default + Send + 'a,
    T: Send + 'a,
    F: Fn(A, T) -> A + Send + 'a,
{
    let f = Arc::new(f);
    move |mut stream| {
        let f = Arc::clone(&f);
        Box::pin(async move {
            let mut acc = A::default();
            while let Some(event) = stream.next().await {
                acc = f(acc, event);
            }
            acc
        })
    }
}

/// Async version of `fold` that allows async folding functions.
#[allow(clippy::type_complexity)]
pub async fn fold_async<'a, A, T, F>(
    f: F,
) -> impl Fn(Pin<Box<dyn Stream<Item = T>>>) -> Pin<Box<dyn Future<Output = A> + 'a>> + Clone
where
    A: Default + Send + 'a,
    T: Send + 'a,
    F: Fn(A, T) -> Pin<Box<dyn Future<Output = A> + Send + 'a>> + Send + 'a,
{
    let f = Arc::new(f);
    move |mut stream| {
        let f = Arc::clone(&f);
        Box::pin(async move {
            let mut acc = A::default();
            while let Some(event) = stream.next().await {
                acc = f(acc, event).await;
            }
            acc
        })
    }
}

////////////////////////////////////////////// unfold //////////////////////////////////////////////

/// Creates an unbounded stream of agent turns with deferred state updates.
///
/// This is the outer layer combinator that turns state into an infinite series of turns.
/// The step function mutates context for the next user turn and returns the updated context.
/// The `update_fn` receives the accumulated `Message` after the stream is fully consumed and
/// takes ownership of the context, allowing updates without cloning.
/// The API stream is created by `make_stream` from the current context.
///
/// The returned stream yields `AccumulatingStream`s which pass through all events while
/// accumulating them into a `Message`. After draining each stream, the message is used
/// to compute the next state via the callback.
#[allow(clippy::type_complexity)]
pub fn unfold<C, F, Fut, U, UFut, S, SFut>(
    initial: C,
    step_fn: F,
    update_fn: U,
    make_stream: S,
) -> impl Stream<Item = Result<AccumulatingStream, Error>>
where
    C: Context + Send + 'static,
    F: Fn(C) -> Fut + Send + 'static,
    Fut: Future<Output = Result<C, Error>> + Send,
    U: Fn(C, Message) -> UFut + Send + Sync + 'static,
    UFut: Future<Output = C> + Send + 'static,
    S: Fn(&C) -> SFut + Send + Sync + 'static,
    SFut: Future<
            Output = Result<
                Pin<Box<dyn Stream<Item = Result<MessageStreamEvent, Error>> + Send>>,
                Error,
            >,
        > + Send,
{
    unfold_until(initial, step_fn, update_fn, make_stream, |_| false)
}

/// Creates a bounded stream of agent turns with deferred state updates.
///
/// Like `unfold`, but stops when the predicate returns `true` on the state.
/// Useful for agents that should terminate after a condition is met.
#[allow(clippy::type_complexity)]
pub fn unfold_until<C, F, Fut, U, UFut, S, SFut, P>(
    initial: C,
    step_fn: F,
    update_fn: U,
    make_stream: S,
    should_stop: P,
) -> impl Stream<Item = Result<AccumulatingStream, Error>>
where
    C: Context + Send + 'static,
    F: Fn(C) -> Fut + Send + 'static,
    Fut: Future<Output = Result<C, Error>> + Send,
    U: Fn(C, Message) -> UFut + Send + Sync + 'static,
    UFut: Future<Output = C> + Send + 'static,
    S: Fn(&C) -> SFut + Send + Sync + 'static,
    SFut: Future<
            Output = Result<
                Pin<Box<dyn Stream<Item = Result<MessageStreamEvent, Error>> + Send>>,
                Error,
            >,
        > + Send,
    P: Fn(&C) -> bool + Send + Sync + 'static,
{
    let no_tools = |_tool_use: &ToolUseBlock| {
        futures::future::ready(Err("tool handling disabled".to_string()))
    };

    unfold_with_tools_core(
        initial,
        step_fn,
        update_fn,
        make_stream,
        no_tools,
        false,
        should_stop,
    )
}

/////////////////////////////////////////// passthrough ////////////////////////////////////////////

/// Inspects stream items without consuming them.
///
/// This combinator executes a side-effect function for each item in the stream (like
/// logging or metrics collection) while passing the original items through unchanged.
/// Unlike [`filter`] or [`filter_map`], `passthrough` cannot modify or filter items—it
/// only observes them.
///
/// # Example
///
/// ```
/// use std::pin::Pin;
/// use std::sync::Arc;
/// use std::sync::atomic::{AtomicUsize, Ordering};
/// use futures::StreamExt;
///
/// # tokio_test::block_on(async {
/// let counter = Arc::new(AtomicUsize::new(0));
/// let counter_clone = counter.clone();
///
/// let stream: Pin<Box<dyn futures::Stream<Item = i32> + Send>> =
///     Box::pin(futures::stream::iter(vec![1, 2, 3]));
///
/// let inspected = claudius::combinators::passthrough(move |_: &i32| {
///     counter_clone.fetch_add(1, Ordering::SeqCst);
/// })(stream);
///
/// let result: Vec<i32> = inspected.collect().await;
/// assert_eq!(result, vec![1, 2, 3]);      // Items unchanged
/// assert_eq!(counter.load(Ordering::SeqCst), 3);  // Side effect observed all items
/// # })
/// ```
#[allow(clippy::type_complexity)]
pub fn passthrough<T, F>(
    inspector: F,
) -> impl Fn(Pin<Box<dyn Stream<Item = T>>>) -> Pin<Box<dyn Stream<Item = T>>> + Clone
where
    T: Send + 'static,
    F: Fn(&T) + Send + Sync + 'static,
{
    let inspector = Arc::new(inspector);
    move |stream| {
        let inspector = Arc::clone(&inspector);
        Box::pin(stream.inspect(move |t| (inspector.as_ref())(t)))
    }
}

/// Async version of `passthrough` that allows async inspection.
///
/// Note: The inspector is called for each item but does not block the stream;
/// items continue flowing while inspection occurs.
#[allow(clippy::type_complexity)]
pub fn passthrough_async<T, F>(
    inspector: F,
) -> impl Fn(Pin<Box<dyn Stream<Item = T>>>) -> Pin<Box<dyn Stream<Item = T>>> + Clone
where
    T: Send + 'static,
    F: for<'a> Fn(&'a T) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>
        + Send
        + Sync
        + 'static,
{
    let inspector = Arc::new(inspector);
    move |stream| {
        let inspector = Arc::clone(&inspector);
        Box::pin(stream.then(move |t| {
            let inspector = Arc::clone(&inspector);
            async move {
                inspector(&t).await;
                t
            }
        }))
    }
}

////////////////////////////////////////// debug_stream /////////////////////////////////////////////

/// Wraps a stream-creating function to print the context before each API call.
///
/// This debugging combinator intercepts calls to create API streams and prints the
/// serialized context as JSON to stderr before delegating to the wrapped function.
/// This is invaluable for debugging tool-use loops to see exactly what messages are
/// being sent to the API at each step.
///
/// # Arguments
///
/// * `label` - A static string label to identify this debug point in output
/// * `make_stream` - The stream-creating function to wrap
///
/// # Output Format
///
/// For each API call, outputs to stderr:
/// ```text
/// === Label ===
/// Message[0]: { "role": "user", "content": [...] }
/// Message[1]: { "role": "assistant", "content": [...] }
/// === End Label ===
/// ```
///
/// # Example
///
/// ```no_run
/// # use claudius::combinators::{debug_stream, client, VecContext, Context};
/// # use claudius::{impl_simple_context, MessageCreateTemplate};
///
/// #[derive(Clone)]
/// struct ChatState {
///     thread: VecContext,
/// }
/// impl_simple_context!(ChatState, thread);
///
/// let template = MessageCreateTemplate::default();
/// let streamer = client::<VecContext>(None);
///
/// // Wrap the client call with debug output
/// let make_stream = debug_stream(
///     "API Call",
///     move |ctx: &ChatState| {
///         let template = template.clone();
///         let ctx = ctx.clone();
///         let streamer = streamer.clone();
///         async move { streamer(template, ctx.thread).await }
///     },
/// );
/// ```
#[allow(clippy::type_complexity)]
pub fn debug_stream<C, S, SFut>(
    label: &'static str,
    make_stream: S,
) -> impl Fn(
    &C,
) -> Pin<
    Box<
        dyn Future<
                Output = Result<
                    Pin<Box<dyn Stream<Item = Result<MessageStreamEvent, Error>> + Send>>,
                    Error,
                >,
            > + Send,
    >,
> + Clone
+ Send
+ Sync
+ 'static
where
    C: Context + Clone + Send + 'static,
    S: Fn(&C) -> SFut + Send + Sync + 'static,
    SFut: Future<
            Output = Result<
                Pin<Box<dyn Stream<Item = Result<MessageStreamEvent, Error>> + Send>>,
                Error,
            >,
        > + Send
        + 'static,
{
    let make_stream = Arc::new(make_stream);
    move |ctx: &C| {
        let make_stream = Arc::clone(&make_stream);
        let ctx = ctx.clone();
        Box::pin(async move {
            let messages = ctx.clone().prepare();
            eprintln!("\n=== {} ===", label);
            for (i, msg) in messages.iter().enumerate() {
                match serde_json::to_string_pretty(msg) {
                    Ok(json) => eprintln!("Message[{}]: {}", i, json),
                    Err(e) => eprintln!("Message[{}]: <serialization error: {}>", i, e),
                }
            }
            eprintln!("=== End {} ===\n", label);
            make_stream(&ctx).await
        }) as Pin<Box<dyn Future<Output = _> + Send>>
    }
}

/////////////////////////////////////// execute_tools_async /////////////////////////////////////////

/// Executes all tool uses in a message concurrently and returns a user message with tool results.
///
/// This helper function processes all `ToolUseBlock`s in a message by calling the provided
/// async handler for each **in parallel**, then packages the results into a `MessageParam`
/// suitable for sending back to the API. Results are returned in the same order as the
/// original tool uses, regardless of completion order.
///
/// # Arguments
///
/// * `msg` - The message containing tool use blocks to execute
/// * `tool_handler` - An async function that executes a tool and returns `Ok(result)` or `Err(error)`
///
/// # Returns
///
/// A `MessageParam` with role `User` containing all tool results as `ToolResultBlock`s.
async fn execute_tools_async<'a, T, TFut>(msg: &'a Message, tool_handler: &T) -> MessageParam
where
    T: Fn(&'a ToolUseBlock) -> TFut,
    TFut: Future<Output = Result<String, String>>,
{
    let tool_uses = extract_tool_uses(msg);

    // Execute all tool handlers concurrently
    let futures: Vec<_> = tool_uses
        .iter()
        .map(|tool_use| async {
            let (content, is_error) = match tool_handler(tool_use).await {
                Ok(result) => (result, false),
                Err(error) => (error, true),
            };
            ToolResultBlock {
                tool_use_id: tool_use.id.clone(),
                cache_control: None,
                content: Some(ToolResultBlockContent::String(content)),
                is_error: Some(is_error),
            }
        })
        .collect();

    let results = futures::future::join_all(futures).await;
    let result_blocks: Vec<ContentBlock> =
        results.into_iter().map(ContentBlock::ToolResult).collect();
    MessageParam::new_with_blocks(result_blocks, crate::MessageRole::User)
}

#[allow(clippy::type_complexity)]
fn unfold_with_tools_core<C, F, Fut, U, UFut, S, SFut, T, TFut, P>(
    initial: C,
    step_fn: F,
    update_fn: U,
    make_stream: S,
    tool_handler: T,
    tools_enabled: bool,
    should_stop: P,
) -> impl Stream<Item = Result<AccumulatingStream, Error>>
where
    C: Context + Send + 'static,
    F: Fn(C) -> Fut + Send + 'static,
    Fut: Future<Output = Result<C, Error>> + Send,
    U: Fn(C, Message) -> UFut + Send + Sync + 'static,
    UFut: Future<Output = C> + Send + 'static,
    S: Fn(&C) -> SFut + Send + Sync + 'static,
    SFut: Future<
            Output = Result<
                Pin<Box<dyn Stream<Item = Result<MessageStreamEvent, Error>> + Send>>,
                Error,
            >,
        > + Send,
    T: for<'a> Fn(&'a ToolUseBlock) -> TFut + Send + Sync + 'static,
    TFut: Future<Output = Result<String, String>> + Send,
    P: Fn(&C) -> bool + Send + Sync + 'static,
{
    let step_fn = Arc::new(step_fn);
    let update_fn = Arc::new(update_fn);
    let make_stream = Arc::new(make_stream);
    let tool_handler = Arc::new(tool_handler);
    let should_stop = Arc::new(should_stop);

    futures::stream::unfold(Some(UnfoldState::Initial(initial)), move |state| {
        let step_fn = Arc::clone(&step_fn);
        let update_fn = Arc::clone(&update_fn);
        let make_stream = Arc::clone(&make_stream);
        let tool_handler = Arc::clone(&tool_handler);
        let should_stop = Arc::clone(&should_stop);
        async move {
            let state = state?;
            match state {
                UnfoldState::Initial(ctx) => {
                    if should_stop(&ctx) {
                        return None;
                    }
                    let ctx = match step_fn(ctx).await {
                        Ok(result) => result,
                        Err(e) => return Some((Err(e), None)),
                    };
                    if should_stop(&ctx) {
                        return None;
                    }
                    match make_stream(&ctx).await {
                        Ok(stream) => {
                            let (acc_stream, message_rx) = AccumulatingStream::new(stream);
                            let next_state = UnfoldState::PendingUserTurn(message_rx, ctx);
                            Some((Ok(acc_stream), Some(next_state)))
                        }
                        Err(e) => Some((Err(e), None)),
                    }
                }
                UnfoldState::PendingUserTurn(rx, ctx) => match rx.await {
                    Ok(msg) => {
                        let mut ctx = update_fn(ctx, msg.clone()).await;

                        if tools_enabled && is_tool_use(&msg) {
                            let tool_result_message =
                                execute_tools_async(&msg, &*tool_handler).await;
                            ctx.push_or_merge_message(tool_result_message);

                            match make_stream(&ctx).await {
                                Ok(stream) => {
                                    let (acc_stream, message_rx) = AccumulatingStream::new(stream);
                                    let next_state =
                                        UnfoldState::PendingToolTurn(message_rx, ctx);
                                    Some((Ok(acc_stream), Some(next_state)))
                                }
                                Err(e) => Some((Err(e), None)),
                            }
                        } else {
                            if should_stop(&ctx) {
                                return None;
                            }
                            let ctx = match step_fn(ctx).await {
                                Ok(result) => result,
                                Err(e) => return Some((Err(e), None)),
                            };
                            if should_stop(&ctx) {
                                return None;
                            }
                            match make_stream(&ctx).await {
                                Ok(stream) => {
                                    let (acc_stream, message_rx) =
                                        AccumulatingStream::new(stream);
                                    let next_state =
                                        UnfoldState::PendingUserTurn(message_rx, ctx);
                                    Some((Ok(acc_stream), Some(next_state)))
                                }
                                Err(e) => Some((Err(e), None)),
                            }
                        }
                    }
                    Err(_) => None,
                },
                UnfoldState::PendingToolTurn(rx, mut ctx) => match rx.await {
                    Ok(msg) => {
                        ctx.push_or_merge_message(msg.clone().into());

                        if tools_enabled && is_tool_use(&msg) {
                            let tool_result_message =
                                execute_tools_async(&msg, &*tool_handler).await;
                            ctx.push_or_merge_message(tool_result_message);

                            match make_stream(&ctx).await {
                                Ok(stream) => {
                                    let (acc_stream, message_rx) = AccumulatingStream::new(stream);
                                    let next_state =
                                        UnfoldState::PendingToolTurn(message_rx, ctx);
                                    Some((Ok(acc_stream), Some(next_state)))
                                }
                                Err(e) => Some((Err(e), None)),
                            }
                        } else {
                            if should_stop(&ctx) {
                                return None;
                            }
                            let ctx = match step_fn(ctx).await {
                                Ok(result) => result,
                                Err(e) => return Some((Err(e), None)),
                            };
                            if should_stop(&ctx) {
                                return None;
                            }
                            match make_stream(&ctx).await {
                                Ok(stream) => {
                                    let (acc_stream, message_rx) =
                                        AccumulatingStream::new(stream);
                                    let next_state =
                                        UnfoldState::PendingUserTurn(message_rx, ctx);
                                    Some((Ok(acc_stream), Some(next_state)))
                                }
                                Err(e) => Some((Err(e), None)),
                            }
                        }
                    }
                    Err(_) => None,
                },
            }
        }
    })
}

//////////////////////////////////////// unfold_with_tools //////////////////////////////////////////

/// Creates a stream of agent turns that handles tool use automatically.
///
/// This is the primary combinator for building tool-using agents. It extends
/// [`unfold_until`] with automatic tool call handling, creating a full agentic loop.
///
/// # Tool Handling Flow
///
/// When the model uses tools:
/// 1. The tool results are computed via `tool_handler` (called synchronously)
/// 2. Results are added to context as a user message with [`crate::ToolResultBlock`]s
/// 3. Another model call is made automatically via `make_stream`
/// 4. This repeats until the model stops using tools (returns `EndTurn`)
///
/// # Key Functions
///
/// - `step_fn`: Called for user turns only. Reads input and mutates context, returning the
///   updated context.
/// - `update_fn`: Called after each assistant response, taking ownership of context to apply
///   updates without cloning.
/// - `make_stream`: Creates the API stream from context. Called for both user and tool turns.
/// - `tool_handler`: Executes a single tool synchronously, returning `Ok(result)` or `Err(error)`.
/// - `should_stop`: Predicate checked before each user turn to terminate the loop.
///
/// # Type Parameters
///
/// - `C`: Context type that holds conversation state. Must implement [`Context`].
/// - `F`: Step function `Fn(C) -> Future<Output = Result<C, Error>>`
/// - `U`: Update function `Fn(C, Message) -> Future<Output = C>`
/// - `S`: Stream factory `Fn(&C) -> Future<Output = Result<Stream, Error>>`
/// - `T`: Tool handler `Fn(&ToolUseBlock) -> Result<String, String>`
/// - `P`: Stop predicate `Fn(&C) -> bool`
///
/// For async tool handlers, see [`unfold_with_tools_async`].
///
/// # Example
///
/// ```no_run
/// # use claudius::combinators::{unfold_with_tools, client, read_user_input, VecContext, Context};
/// # use claudius::{impl_simple_context, Error, Message, MessageParam, MessageCreateTemplate, push_or_merge_message};
/// # use futures::StreamExt;
/// # use std::future::Future;
///
/// #[derive(Clone)]
/// struct ChatState {
///     thread: VecContext,
///     should_quit: bool,
/// }
/// impl_simple_context!(ChatState, thread);
///
/// #[tokio::main]
/// async fn main() -> Result<(), Error> {
///     let template = MessageCreateTemplate::default();
///     let streamer = client::<VecContext>(None);
///
///     let agent = unfold_with_tools(
///         ChatState { thread: VecContext(vec![]), should_quit: false },
///         // step_fn: called for user turns only
///         |mut state: ChatState| async move {
///             let user_input = match read_user_input() {
///                 Some(input) => input,
///                 None => {
///                     state.should_quit = true;
///                     return Ok(state);
///                 }
///             };
///             push_or_merge_message(&mut state.thread.0, MessageParam::user(&user_input));
///             Ok(state)
///         },
///         // update_fn: update context after each assistant response
///         |mut state: ChatState, msg: Message| async move {
///             push_or_merge_message(&mut state.thread.0, msg.into());
///             state
///         },
///         // make_stream: creates API call from context
///         {
///             let template = template.clone();
///             move |ctx: &ChatState| {
///                 let template = template.clone();
///                 let ctx = ctx.clone();
///                 let streamer = streamer.clone();
///                 async move { streamer(template, ctx.thread).await }
///             }
///         },
///         // tool_handler: execute tools synchronously
///         |tool_use| {
///             match tool_use.name.as_str() {
///                 "get_weather" => Ok("Sunny, 72°F".to_string()),
///                 _ => Err(format!("Unknown tool: {}", tool_use.name)),
///             }
///         },
///         |state| state.should_quit,
///     );
///
///     // Consume the agent stream
///     futures::pin_mut!(agent);
///     while let Some(result) = agent.next().await {
///         let mut stream = result?;
///         while let Some(_event) = stream.next().await {
///             // Display streaming events
///         }
///     }
///     Ok(())
/// }
/// ```
#[allow(clippy::type_complexity)]
pub fn unfold_with_tools<C, F, Fut, U, UFut, S, SFut, T, P>(
    initial: C,
    step_fn: F,
    update_fn: U,
    make_stream: S,
    tool_handler: T,
    should_stop: P,
) -> impl Stream<Item = Result<AccumulatingStream, Error>>
where
    C: Context + Send + 'static,
    F: Fn(C) -> Fut + Send + 'static,
    Fut: Future<Output = Result<C, Error>> + Send,
    U: Fn(C, Message) -> UFut + Send + Sync + 'static,
    UFut: Future<Output = C> + Send + 'static,
    S: Fn(&C) -> SFut + Send + Sync + 'static,
    SFut: Future<
            Output = Result<
                Pin<Box<dyn Stream<Item = Result<MessageStreamEvent, Error>> + Send>>,
                Error,
            >,
        > + Send,
    T: Fn(&ToolUseBlock) -> Result<String, String> + Send + Sync + 'static,
    P: Fn(&C) -> bool + Send + Sync + 'static,
{
    let tool_handler = move |tool_use: &ToolUseBlock| {
        let result = tool_handler(tool_use);
        async move { result }
    };

    unfold_with_tools_core(
        initial,
        step_fn,
        update_fn,
        make_stream,
        tool_handler,
        true,
        should_stop,
    )
}

/// Async version of `unfold_with_tools` for async tool handlers.
#[allow(clippy::type_complexity)]
pub fn unfold_with_tools_async<C, F, Fut, U, UFut, S, SFut, T, TFut, P>(
    initial: C,
    step_fn: F,
    update_fn: U,
    make_stream: S,
    tool_handler: T,
    should_stop: P,
) -> impl Stream<Item = Result<AccumulatingStream, Error>>
where
    C: Context + Send + 'static,
    F: Fn(C) -> Fut + Send + 'static,
    Fut: Future<Output = Result<C, Error>> + Send,
    U: Fn(C, Message) -> UFut + Send + Sync + 'static,
    UFut: Future<Output = C> + Send + 'static,
    S: Fn(&C) -> SFut + Send + Sync + 'static,
    SFut: Future<
            Output = Result<
                Pin<Box<dyn Stream<Item = Result<MessageStreamEvent, Error>> + Send>>,
                Error,
            >,
        > + Send,
    T: for<'a> Fn(&'a ToolUseBlock) -> TFut + Send + Sync + 'static,
    TFut: Future<Output = Result<String, String>> + Send,
    P: Fn(&C) -> bool + Send + Sync + 'static,
{
    unfold_with_tools_core(
        initial,
        step_fn,
        update_fn,
        make_stream,
        tool_handler,
        true,
        should_stop,
    )
}

/// Internal state for unfold turns (tool or non-tool).
#[allow(clippy::type_complexity)]
enum UnfoldState<C> {
    /// Initial state or ready for a new user turn.
    Initial(C),
    /// Waiting for a user-initiated turn to complete.
    PendingUserTurn(tokio::sync::oneshot::Receiver<Message>, C),
    /// Waiting for a tool-continuation turn to complete (no update_fn needed, context managed internally).
    PendingToolTurn(tokio::sync::oneshot::Receiver<Message>, C),
}

////////////////////////////////////////// Tool Helpers /////////////////////////////////////////////

/// Extracts all [`crate::ToolUseBlock`]s from a [`crate::Message`].
///
/// Returns a vector of references to tool use blocks in the order they appear
/// in the message content. This is useful for processing tool calls after
/// receiving a response with `StopReason::ToolUse`.
///
/// # Example
///
/// ```
/// # use claudius::{ContentBlock, KnownModel, Message, Model, ToolUseBlock, Usage};
/// # use claudius::combinators::extract_tool_uses;
///
/// let mut message = Message::new(
///     "msg_test".to_string(),
///     vec![],
///     Model::Known(KnownModel::Claude37Sonnet20250219),
///     Usage::new(10, 20),
/// );
/// message.content = vec![
///     ContentBlock::ToolUse(ToolUseBlock::new("tool_1", "search", serde_json::json!({"q": "test"}))),
///     ContentBlock::ToolUse(ToolUseBlock::new("tool_2", "calculate", serde_json::json!({"x": 42}))),
/// ];
///
/// let tool_uses = extract_tool_uses(&message);
/// assert_eq!(tool_uses.len(), 2);
/// assert_eq!(tool_uses[0].name, "search");
/// assert_eq!(tool_uses[1].name, "calculate");
/// ```
pub fn extract_tool_uses(message: &Message) -> Vec<&ToolUseBlock> {
    message
        .content
        .iter()
        .filter_map(|block| {
            if let ContentBlock::ToolUse(tool_use) = block {
                Some(tool_use)
            } else {
                None
            }
        })
        .collect()
}

/// Returns `true` if the message stopped because the model wants to use tools.
///
/// This is a convenience predicate for checking [`crate::StopReason::ToolUse`].
/// When this returns `true`, use [`extract_tool_uses`] to get the tool calls and
/// [`tool_results_for_message`] or [`tool_results_for_message_async`] to execute them.
///
/// # Example
///
/// ```
/// # use claudius::{KnownModel, Message, Model, StopReason, Usage};
/// # use claudius::combinators::is_tool_use;
///
/// let mut message = Message::new(
///     "msg_test".to_string(),
///     vec![],
///     Model::Known(KnownModel::Claude37Sonnet20250219),
///     Usage::new(10, 20),
/// );
///
/// // Message without stop reason
/// assert!(!is_tool_use(&message));
///
/// // Message with EndTurn stop reason
/// message.stop_reason = Some(StopReason::EndTurn);
/// assert!(!is_tool_use(&message));
///
/// // Message with ToolUse stop reason
/// message.stop_reason = Some(StopReason::ToolUse);
/// assert!(is_tool_use(&message));
/// ```
pub fn is_tool_use(message: &Message) -> bool {
    message.stop_reason == Some(StopReason::ToolUse)
}

/// Creates tool result blocks by executing a handler function for each tool use.
///
/// This is a synchronous version that maps each [`crate::ToolUseBlock`] in the message
/// to a [`crate::ToolResultBlock`] using the provided handler function. The handler
/// receives the tool use and returns a `Result<String, String>` where `Ok` indicates
/// success and `Err` indicates an error result.
///
/// For async tool handlers, see [`tool_results_for_message_async`].
///
/// # Example
///
/// ```
/// # use claudius::{ContentBlock, KnownModel, Message, Model, ToolUseBlock, Usage};
/// # use claudius::combinators::tool_results_for_message;
///
/// let mut message = Message::new(
///     "msg_test".to_string(),
///     vec![],
///     Model::Known(KnownModel::Claude37Sonnet20250219),
///     Usage::new(10, 20),
/// );
/// message.content = vec![
///     ContentBlock::ToolUse(ToolUseBlock::new("tool_1", "get_weather", serde_json::json!({}))),
///     ContentBlock::ToolUse(ToolUseBlock::new("tool_2", "unknown", serde_json::json!({}))),
/// ];
///
/// let results = tool_results_for_message(&message, |tool_use| {
///     match tool_use.name.as_str() {
///         "get_weather" => Ok("Sunny, 72°F".to_string()),
///         _ => Err(format!("Unknown tool: {}", tool_use.name)),
///     }
/// });
///
/// assert_eq!(results.len(), 2);
/// assert_eq!(results[0].is_error, Some(false));
/// assert_eq!(results[1].is_error, Some(true));
/// ```
pub fn tool_results_for_message<F>(message: &Message, handler: F) -> Vec<ToolResultBlock>
where
    F: Fn(&ToolUseBlock) -> Result<String, String>,
{
    extract_tool_uses(message)
        .into_iter()
        .map(|tool_use| {
            let (content, is_error) = match handler(tool_use) {
                Ok(result) => (result, false),
                Err(error) => (error, true),
            };
            ToolResultBlock {
                tool_use_id: tool_use.id.clone(),
                cache_control: None,
                content: Some(ToolResultBlockContent::String(content)),
                is_error: Some(is_error),
            }
        })
        .collect()
}

/// Async version of [`tool_results_for_message`] that allows async tool execution.
///
/// This executes each tool handler **concurrently** using [`futures::future::join_all`]
/// and collects the results. Results are returned in the same order as the original
/// tool uses, regardless of completion order.
///
/// # Example
///
/// ```
/// # use claudius::{ContentBlock, KnownModel, Message, Model, ToolUseBlock, Usage};
/// # use claudius::combinators::tool_results_for_message_async;
///
/// # tokio_test::block_on(async {
/// let mut message = Message::new(
///     "msg_test".to_string(),
///     vec![],
///     Model::Known(KnownModel::Claude37Sonnet20250219),
///     Usage::new(10, 20),
/// );
/// message.content = vec![
///     ContentBlock::ToolUse(ToolUseBlock::new("tool_1", "search", serde_json::json!({"q": "rust"}))),
///     ContentBlock::ToolUse(ToolUseBlock::new("tool_2", "search", serde_json::json!({"q": "async"}))),
/// ];
///
/// let results = tool_results_for_message_async(&message, |tool_use| {
///     let query = tool_use.input["q"].as_str().unwrap_or("").to_string();
///     async move {
///         // Simulated async operation
///         Ok(format!("Results for: {}", query))
///     }
/// }).await;
///
/// assert_eq!(results.len(), 2);
/// assert_eq!(results[0].tool_use_id, "tool_1");
/// assert_eq!(results[1].tool_use_id, "tool_2");
/// # })
/// ```
pub async fn tool_results_for_message_async<'a, F, Fut>(
    message: &'a Message,
    handler: F,
) -> Vec<ToolResultBlock>
where
    F: Fn(&'a ToolUseBlock) -> Fut,
    Fut: Future<Output = Result<String, String>>,
{
    let tool_uses = extract_tool_uses(message);

    // Execute all tool handlers concurrently
    let futures: Vec<_> = tool_uses
        .iter()
        .map(|tool_use| async {
            let (content, is_error) = match handler(tool_use).await {
                Ok(result) => (result, false),
                Err(error) => (error, true),
            };
            ToolResultBlock {
                tool_use_id: tool_use.id.clone(),
                cache_control: None,
                content: Some(ToolResultBlockContent::String(content)),
                is_error: Some(is_error),
            }
        })
        .collect();

    futures::future::join_all(futures).await
}

//////////////////////////////////////////// to_message ////////////////////////////////////////////

/// Accumulates a stream of [`crate::MessageStreamEvent`]s into a complete [`crate::Message`].
///
/// This combinator collects all events from a streaming response and reconstructs
/// the full message by:
/// - Extracting the initial message structure from `MessageStart`
/// - Building content blocks from `ContentBlockStart`, `ContentBlockDelta`, and `ContentBlockStop`
/// - Updating stop reason and usage from `MessageDelta`
///
/// This is useful when you need the complete message without streaming individual tokens
/// to the user.
///
/// # Example
///
/// ```
/// use std::pin::Pin;
/// use futures::stream;
/// # use claudius::{
/// #     ContentBlock, ContentBlockDelta, ContentBlockDeltaEvent, ContentBlockStartEvent,
/// #     ContentBlockStopEvent, Error, KnownModel, Message, MessageDelta, MessageDeltaEvent,
/// #     MessageDeltaUsage, MessageStartEvent, MessageStopEvent, MessageStreamEvent, Model,
/// #     StopReason, TextBlock, TextDelta, Usage,
/// # };
/// # use claudius::combinators::to_message;
///
/// # tokio_test::block_on(async {
/// let events = vec![
///     Ok(MessageStreamEvent::MessageStart(MessageStartEvent::new(
///         Message::new("msg_1".to_string(), vec![], Model::Known(KnownModel::Claude37Sonnet20250219), Usage::new(10, 20))
///     ))),
///     Ok(MessageStreamEvent::ContentBlockStart(ContentBlockStartEvent::new(
///         ContentBlock::Text(TextBlock::new("")), 0
///     ))),
///     Ok(MessageStreamEvent::ContentBlockDelta(ContentBlockDeltaEvent::new(
///         ContentBlockDelta::TextDelta(TextDelta::new("Hello".to_string())), 0
///     ))),
///     Ok(MessageStreamEvent::ContentBlockStop(ContentBlockStopEvent { index: 0 })),
///     Ok(MessageStreamEvent::MessageDelta(MessageDeltaEvent::new(
///         MessageDelta::new().with_stop_reason(StopReason::EndTurn),
///         MessageDeltaUsage::new(5)
///     ))),
///     Ok(MessageStreamEvent::MessageStop(MessageStopEvent {})),
/// ];
///
/// let stream: Pin<Box<dyn futures::Stream<Item = Result<MessageStreamEvent, Error>> + Send>> =
///     Box::pin(stream::iter(events));
///
/// let message = to_message()(stream).await.unwrap();
/// assert_eq!(message.id, "msg_1");
/// assert_eq!(message.content.len(), 1);
/// # })
/// ```
#[allow(clippy::type_complexity)]
pub fn to_message() -> impl Fn(
    Pin<Box<dyn Stream<Item = Result<MessageStreamEvent, Error>> + Send>>,
) -> Pin<Box<dyn Future<Output = Result<Message, Error>> + Send + 'static>> {
    |stream: Pin<Box<dyn Stream<Item = Result<MessageStreamEvent, Error>> + Send>>| {
        Box::pin(async move {
            let (mut acc_stream, rx) = AccumulatingStream::new(stream);
            // Drain the stream to accumulate all events
            while let Some(result) = acc_stream.next().await {
                // Propagate any errors from the stream
                result?;
            }
            // Retrieve the accumulated message
            rx.await.map_err(|_| Error::Streaming {
                message: "Failed to receive accumulated message".to_string(),
                source: None,
            })
        })
    }
}

//////////////////////////////////////// AccumulatingStream /////////////////////////////////////////

/// A stream wrapper that passes through `MessageStreamEvent`s while accumulating them into a
/// complete `Message`.
///
/// This allows streaming tokens to the user while simultaneously building the final message
/// without buffering. When the stream is fully drained, the accumulated message is sent via
/// a oneshot channel returned from `new()`.
pub struct AccumulatingStream {
    inner: Pin<Box<dyn Stream<Item = Result<MessageStreamEvent, Error>> + Send>>,
    message_tx: Option<tokio::sync::oneshot::Sender<Message>>,
    message: Option<Message>,
    content_blocks: Vec<ContentBlockBuilder>,
}

impl AccumulatingStream {
    /// Wraps a `MessageStreamEvent` stream to accumulate events into a `Message`.
    ///
    /// Returns the stream and a receiver that will contain the accumulated `Message` once the
    /// stream is fully drained.
    pub fn new(
        stream: Pin<Box<dyn Stream<Item = Result<MessageStreamEvent, Error>> + Send>>,
    ) -> (Self, tokio::sync::oneshot::Receiver<Message>) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let this = Self {
            inner: stream,
            message_tx: Some(tx),
            message: None,
            content_blocks: Vec::new(),
        };
        (this, rx)
    }

    fn accumulate_event(&mut self, event: &MessageStreamEvent) {
        match event {
            MessageStreamEvent::MessageStart(start) => {
                self.message = Some(start.message.clone());
            }
            MessageStreamEvent::ContentBlockStart(start) => {
                let idx = start.index;
                while self.content_blocks.len() <= idx {
                    self.content_blocks.push(ContentBlockBuilder::Empty);
                }
                self.content_blocks[idx] =
                    ContentBlockBuilder::from_content_block(start.content_block.clone());
            }
            MessageStreamEvent::ContentBlockDelta(delta_event) => {
                let idx = delta_event.index;
                if idx < self.content_blocks.len() {
                    self.content_blocks[idx].apply_delta(delta_event.delta.clone());
                }
            }
            MessageStreamEvent::ContentBlockStop(_) => {}
            MessageStreamEvent::MessageDelta(delta_event) => {
                if let Some(ref mut msg) = self.message {
                    if delta_event.delta.stop_reason.is_some() {
                        msg.stop_reason = delta_event.delta.stop_reason;
                    }
                    if delta_event.delta.stop_sequence.is_some() {
                        msg.stop_sequence = delta_event.delta.stop_sequence.clone();
                    }
                    if let Some(input_tokens) = delta_event.usage.input_tokens {
                        msg.usage.input_tokens = input_tokens;
                    }
                    msg.usage.output_tokens = delta_event.usage.output_tokens;
                    if delta_event.usage.cache_creation_input_tokens.is_some() {
                        msg.usage.cache_creation_input_tokens =
                            delta_event.usage.cache_creation_input_tokens;
                    }
                    if delta_event.usage.cache_read_input_tokens.is_some() {
                        msg.usage.cache_read_input_tokens =
                            delta_event.usage.cache_read_input_tokens;
                    }
                }
            }
            MessageStreamEvent::MessageStop(_) => {}
            MessageStreamEvent::Ping => {}
        }
    }

    fn finalize(&mut self) -> Option<Message> {
        let mut msg = self.message.take()?;
        msg.content = std::mem::take(&mut self.content_blocks)
            .into_iter()
            .filter_map(|b| b.build())
            .collect();
        Some(msg)
    }
}

impl Stream for AccumulatingStream {
    type Item = Result<MessageStreamEvent, Error>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match self.inner.as_mut().poll_next(cx) {
            std::task::Poll::Ready(Some(Ok(event))) => {
                self.accumulate_event(&event);
                std::task::Poll::Ready(Some(Ok(event)))
            }
            std::task::Poll::Ready(Some(Err(e))) => std::task::Poll::Ready(Some(Err(e))),
            std::task::Poll::Ready(None) => {
                if let Some(tx) = self.message_tx.take()
                    && let Some(msg) = self.finalize()
                {
                    let _ = tx.send(msg);
                }
                std::task::Poll::Ready(None)
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

////////////////////////////////////////// ContentBlockBuilder //////////////////////////////////////

/// Builder for accumulating content block deltas into a complete content block.
enum ContentBlockBuilder {
    /// No content block has been started.
    Empty,
    /// Building a text block.
    Text {
        text: String,
        citations: Option<Vec<crate::TextCitation>>,
        cache_control: Option<crate::CacheControlEphemeral>,
    },
    /// Building a tool use block.
    ToolUse {
        id: String,
        name: String,
        input_json: String,
        cache_control: Option<crate::CacheControlEphemeral>,
    },
    /// Building a server tool use block.
    ServerToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
        cache_control: Option<crate::CacheControlEphemeral>,
    },
    /// Building a thinking block.
    Thinking { thinking: String, signature: String },
    /// A complete block that doesn't need delta accumulation.
    Complete(crate::ContentBlock),
}

impl ContentBlockBuilder {
    fn from_content_block(block: crate::ContentBlock) -> Self {
        match block {
            crate::ContentBlock::Text(text_block) => ContentBlockBuilder::Text {
                text: text_block.text,
                citations: text_block.citations,
                cache_control: text_block.cache_control,
            },
            crate::ContentBlock::ToolUse(tool_use) => ContentBlockBuilder::ToolUse {
                id: tool_use.id,
                name: tool_use.name,
                // Start with empty string - InputJsonDelta events will build up the complete JSON.
                // The ContentBlockStart event sends input as {} which would corrupt accumulation.
                input_json: String::new(),
                cache_control: tool_use.cache_control,
            },
            crate::ContentBlock::ServerToolUse(server_tool_use) => {
                ContentBlockBuilder::ServerToolUse {
                    id: server_tool_use.id,
                    name: server_tool_use.name,
                    input: server_tool_use.input,
                    cache_control: server_tool_use.cache_control,
                }
            }
            crate::ContentBlock::Thinking(thinking) => ContentBlockBuilder::Thinking {
                thinking: thinking.thinking,
                signature: thinking.signature,
            },
            other => ContentBlockBuilder::Complete(other),
        }
    }

    fn apply_delta(&mut self, delta: ContentBlockDelta) {
        match (self, delta) {
            (ContentBlockBuilder::Text { text, .. }, ContentBlockDelta::TextDelta(text_delta)) => {
                text.push_str(&text_delta.text);
            }
            (
                ContentBlockBuilder::Text { citations, .. },
                ContentBlockDelta::CitationsDelta(citations_delta),
            ) => {
                let citation = match citations_delta.citation {
                    crate::Citation::CharLocation(loc) => crate::TextCitation::CharLocation(loc),
                    crate::Citation::PageLocation(loc) => crate::TextCitation::PageLocation(loc),
                    crate::Citation::ContentBlockLocation(loc) => {
                        crate::TextCitation::ContentBlockLocation(loc)
                    }
                    crate::Citation::WebSearchResultLocation(loc) => {
                        crate::TextCitation::WebSearchResultLocation(loc)
                    }
                };
                citations.get_or_insert_with(Vec::new).push(citation);
            }
            (
                ContentBlockBuilder::ToolUse { input_json, .. },
                ContentBlockDelta::InputJsonDelta(json_delta),
            ) => {
                input_json.push_str(&json_delta.partial_json);
            }
            (
                ContentBlockBuilder::Thinking { thinking, .. },
                ContentBlockDelta::ThinkingDelta(thinking_delta),
            ) => {
                thinking.push_str(&thinking_delta.thinking);
            }
            (
                ContentBlockBuilder::Thinking { signature, .. },
                ContentBlockDelta::SignatureDelta(sig_delta),
            ) => {
                signature.push_str(&sig_delta.signature);
            }
            _ => {
                // Mismatched delta type; ignore.
            }
        }
    }

    fn build(self) -> Option<crate::ContentBlock> {
        match self {
            ContentBlockBuilder::Empty => None,
            ContentBlockBuilder::Text {
                text,
                citations,
                cache_control,
            } => Some(crate::ContentBlock::Text(crate::TextBlock {
                text,
                citations,
                cache_control,
            })),
            ContentBlockBuilder::ToolUse {
                id,
                name,
                input_json,
                cache_control,
            } => {
                let input = serde_json::from_str(&input_json).unwrap_or(serde_json::Value::Null);
                Some(crate::ContentBlock::ToolUse(crate::ToolUseBlock {
                    id,
                    name,
                    input,
                    cache_control,
                }))
            }
            ContentBlockBuilder::ServerToolUse {
                id,
                name,
                input,
                cache_control,
            } => Some(crate::ContentBlock::ServerToolUse(
                crate::ServerToolUseBlock {
                    id,
                    name,
                    input,
                    cache_control,
                },
            )),
            ContentBlockBuilder::Thinking {
                thinking,
                signature,
            } => Some(crate::ContentBlock::Thinking(crate::ThinkingBlock {
                thinking,
                signature,
            })),
            ContentBlockBuilder::Complete(block) => Some(block),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::pin::Pin;

    use futures::StreamExt;
    use futures::stream;

    use crate::{
        ContentBlock, ContentBlockDelta, ContentBlockDeltaEvent, ContentBlockStartEvent,
        ContentBlockStopEvent, InputJsonDelta, KnownModel, Message, MessageDelta,
        MessageDeltaEvent, MessageDeltaUsage, MessageParam, MessageStartEvent, MessageStopEvent,
        MessageStreamEvent, Model, SignatureDelta, StopReason, TextBlock, TextDelta, ThinkingBlock,
        ThinkingDelta, ToolUseBlock, Usage,
    };

    use super::{
        AccumulatingStream, ContentBlockBuilder, Context, VecContext, extract_tool_uses, filter,
        filter_map, fold, is_tool_use, passthrough, tool_results_for_message,
    };

    // ================================
    // Test Helper Functions
    // ================================

    /// Creates a minimal Message for testing.
    fn make_test_message() -> Message {
        Message::new(
            "msg_test".to_string(),
            vec![],
            Model::Known(KnownModel::Claude37Sonnet20250219),
            Usage::new(10, 20),
        )
    }

    /// Creates a MessageStartEvent with the given message.
    fn make_message_start(message: Message) -> MessageStreamEvent {
        MessageStreamEvent::MessageStart(MessageStartEvent::new(message))
    }

    /// Creates a ContentBlockStartEvent with a text block.
    fn make_content_block_start_text(index: usize, text: &str) -> MessageStreamEvent {
        MessageStreamEvent::ContentBlockStart(ContentBlockStartEvent::new(
            ContentBlock::Text(TextBlock::new(text)),
            index,
        ))
    }

    /// Creates a ContentBlockStartEvent with a tool use block.
    fn make_content_block_start_tool_use(index: usize, id: &str, name: &str) -> MessageStreamEvent {
        MessageStreamEvent::ContentBlockStart(ContentBlockStartEvent::new(
            ContentBlock::ToolUse(ToolUseBlock::new(id, name, serde_json::json!({}))),
            index,
        ))
    }

    /// Creates a ContentBlockStartEvent with a thinking block.
    fn make_content_block_start_thinking(index: usize) -> MessageStreamEvent {
        MessageStreamEvent::ContentBlockStart(ContentBlockStartEvent::new(
            ContentBlock::Thinking(ThinkingBlock::new("", "")),
            index,
        ))
    }

    /// Creates a ContentBlockDeltaEvent with a text delta.
    fn make_text_delta(index: usize, text: &str) -> MessageStreamEvent {
        MessageStreamEvent::ContentBlockDelta(ContentBlockDeltaEvent::new(
            ContentBlockDelta::TextDelta(TextDelta::new(text.to_string())),
            index,
        ))
    }

    /// Creates a ContentBlockDeltaEvent with an input JSON delta.
    fn make_input_json_delta(index: usize, json: &str) -> MessageStreamEvent {
        MessageStreamEvent::ContentBlockDelta(ContentBlockDeltaEvent::new(
            ContentBlockDelta::InputJsonDelta(InputJsonDelta::new(json.to_string())),
            index,
        ))
    }

    /// Creates a ContentBlockDeltaEvent with a thinking delta.
    fn make_thinking_delta(index: usize, thinking: &str) -> MessageStreamEvent {
        MessageStreamEvent::ContentBlockDelta(ContentBlockDeltaEvent::new(
            ContentBlockDelta::ThinkingDelta(ThinkingDelta::new(thinking.to_string())),
            index,
        ))
    }

    /// Creates a ContentBlockDeltaEvent with a signature delta.
    fn make_signature_delta(index: usize, signature: &str) -> MessageStreamEvent {
        MessageStreamEvent::ContentBlockDelta(ContentBlockDeltaEvent::new(
            ContentBlockDelta::SignatureDelta(SignatureDelta::new(signature.to_string())),
            index,
        ))
    }

    /// Creates a ContentBlockStopEvent.
    fn make_content_block_stop(index: usize) -> MessageStreamEvent {
        MessageStreamEvent::ContentBlockStop(ContentBlockStopEvent { index })
    }

    /// Creates a MessageDeltaEvent with optional stop reason.
    fn make_message_delta(
        stop_reason: Option<StopReason>,
        output_tokens: i32,
    ) -> MessageStreamEvent {
        let mut delta = MessageDelta::new();
        if let Some(reason) = stop_reason {
            delta = delta.with_stop_reason(reason);
        }
        MessageStreamEvent::MessageDelta(MessageDeltaEvent::new(
            delta,
            MessageDeltaUsage::new(output_tokens),
        ))
    }

    /// Creates a MessageStopEvent.
    fn make_message_stop() -> MessageStreamEvent {
        MessageStreamEvent::MessageStop(MessageStopEvent {})
    }

    // ================================
    // VecContext Tests
    // ================================

    #[test]
    fn vec_context_default_is_empty() {
        let ctx = VecContext::default();
        assert_eq!(ctx, VecContext(vec![]));
    }

    #[test]
    fn vec_context_prepare_returns_inner_vec() {
        let messages = vec![
            MessageParam::user("Hello"),
            MessageParam::assistant("Hi there!"),
        ];
        let ctx = VecContext(messages.clone());
        let prepared = ctx.prepare();
        assert_eq!(prepared.len(), 2);
        println!(
            "vec_context_prepare_returns_inner_vec: prepared {} messages",
            prepared.len()
        );
    }

    #[test]
    fn vec_context_clone() {
        let messages = vec![MessageParam::user("Test")];
        let ctx = VecContext(messages);
        let cloned = ctx.clone();
        assert_eq!(ctx, cloned);
    }

    // ================================
    // Tuple Context Tests
    // ================================

    #[test]
    fn tuple_context_single_element() {
        let ctx1 = VecContext(vec![MessageParam::user("Hello")]);
        let tuple_ctx = (ctx1,);
        let prepared = tuple_ctx.prepare();
        assert_eq!(prepared.len(), 1);
        println!(
            "tuple_context_single_element: prepared {} messages",
            prepared.len()
        );
    }

    #[test]
    fn tuple_context_two_elements_merges_messages() {
        let ctx1 = VecContext(vec![MessageParam::user("Part 1")]);
        let ctx2 = VecContext(vec![MessageParam::assistant("Response")]);
        let tuple_ctx = (ctx1, ctx2);
        let prepared = tuple_ctx.prepare();
        // User message followed by assistant message should remain separate
        assert_eq!(prepared.len(), 2);
        println!(
            "tuple_context_two_elements_merges_messages: prepared {} messages",
            prepared.len()
        );
    }

    #[test]
    fn tuple_context_three_elements() {
        let ctx1 = VecContext(vec![MessageParam::user("Q1")]);
        let ctx2 = VecContext(vec![MessageParam::assistant("A1")]);
        let ctx3 = VecContext(vec![MessageParam::user("Q2")]);
        let tuple_ctx = (ctx1, ctx2, ctx3);
        let prepared = tuple_ctx.prepare();
        assert_eq!(prepared.len(), 3);
        println!(
            "tuple_context_three_elements: prepared {} messages",
            prepared.len()
        );
    }

    // ================================
    // filter_map Combinator Tests
    // ================================

    #[tokio::test]
    async fn filter_map_transforms_all_items() {
        let stream: Pin<Box<dyn futures::Stream<Item = i32>>> =
            Box::pin(stream::iter(vec![1, 2, 3, 4, 5]));
        let filtered = filter_map(|x| Some(x * 2))(stream);
        let result: Vec<i32> = filtered.collect().await;
        assert_eq!(result, vec![2, 4, 6, 8, 10]);
    }

    #[tokio::test]
    async fn filter_map_filters_none_values() {
        let stream: Pin<Box<dyn futures::Stream<Item = i32>>> =
            Box::pin(stream::iter(vec![1, 2, 3, 4, 5]));
        // Only keep even numbers and double them
        let filtered = filter_map(|x| if x % 2 == 0 { Some(x * 2) } else { None })(stream);
        let result: Vec<i32> = filtered.collect().await;
        assert_eq!(result, vec![4, 8]);
    }

    #[tokio::test]
    async fn filter_map_empty_stream() {
        let stream: Pin<Box<dyn futures::Stream<Item = i32>>> = Box::pin(stream::iter(vec![]));
        let filtered = filter_map(|x: i32| Some(x * 2))(stream);
        let result: Vec<i32> = filtered.collect().await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn filter_map_all_filtered_out() {
        let stream: Pin<Box<dyn futures::Stream<Item = i32>>> =
            Box::pin(stream::iter(vec![1, 3, 5, 7]));
        // Filter out all odd numbers (all items)
        let filtered = filter_map(|x| if x % 2 == 0 { Some(x) } else { None })(stream);
        let result: Vec<i32> = filtered.collect().await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn filter_map_type_conversion() {
        let stream: Pin<Box<dyn futures::Stream<Item = i32>>> =
            Box::pin(stream::iter(vec![1, 2, 3]));
        // Convert i32 to String
        let filtered = filter_map(|x| Some(format!("num:{}", x)))(stream);
        let result: Vec<String> = filtered.collect().await;
        assert_eq!(result, vec!["num:1", "num:2", "num:3"]);
    }

    // ================================
    // filter Combinator Tests
    // ================================

    #[tokio::test]
    async fn filter_keeps_matching_items() {
        let stream: Pin<Box<dyn futures::Stream<Item = i32>>> =
            Box::pin(stream::iter(vec![1, 2, 3, 4, 5, 6]));
        let filtered = filter(|x| x % 2 == 0)(stream);
        let result: Vec<i32> = filtered.collect().await;
        assert_eq!(result, vec![2, 4, 6]);
    }

    #[tokio::test]
    async fn filter_keeps_all_when_predicate_always_true() {
        let stream: Pin<Box<dyn futures::Stream<Item = i32>>> =
            Box::pin(stream::iter(vec![1, 2, 3]));
        let filtered = filter(|_| true)(stream);
        let result: Vec<i32> = filtered.collect().await;
        assert_eq!(result, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn filter_removes_all_when_predicate_always_false() {
        let stream: Pin<Box<dyn futures::Stream<Item = i32>>> =
            Box::pin(stream::iter(vec![1, 2, 3]));
        let filtered = filter(|_| false)(stream);
        let result: Vec<i32> = filtered.collect().await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn filter_empty_stream() {
        let stream: Pin<Box<dyn futures::Stream<Item = i32>>> = Box::pin(stream::iter(vec![]));
        let filtered = filter(|_: &i32| true)(stream);
        let result: Vec<i32> = filtered.collect().await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn filter_boundary_values() {
        let stream: Pin<Box<dyn futures::Stream<Item = i32>>> =
            Box::pin(stream::iter(vec![0, 42, 43, 100]));
        // Keep values > 42
        let filtered = filter(|x| *x > 42)(stream);
        let result: Vec<i32> = filtered.collect().await;
        assert_eq!(result, vec![43, 100]);
    }

    // ================================
    // fold Combinator Tests
    // ================================

    #[tokio::test]
    async fn fold_sums_values() {
        let stream: Pin<Box<dyn futures::Stream<Item = i32>>> =
            Box::pin(stream::iter(vec![1, 2, 3, 4, 5]));
        let folder = fold(|acc: i32, x| acc + x).await;
        let result = folder(stream).await;
        assert_eq!(result, 15);
    }

    #[tokio::test]
    async fn fold_empty_stream_returns_default() {
        let stream: Pin<Box<dyn futures::Stream<Item = i32>>> = Box::pin(stream::iter(vec![]));
        let folder = fold(|acc: i32, x| acc + x).await;
        let result = folder(stream).await;
        assert_eq!(result, 0); // i32::default() is 0
    }

    #[tokio::test]
    async fn fold_collects_into_vec() {
        let stream: Pin<Box<dyn futures::Stream<Item = i32>>> =
            Box::pin(stream::iter(vec![1, 2, 3]));
        let folder = fold(|mut acc: Vec<i32>, x| {
            acc.push(x);
            acc
        })
        .await;
        let result = folder(stream).await;
        assert_eq!(result, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn fold_string_concatenation() {
        let stream: Pin<Box<dyn futures::Stream<Item = &str>>> =
            Box::pin(stream::iter(vec!["Hello", " ", "World"]));
        let folder = fold(|mut acc: String, x| {
            acc.push_str(x);
            acc
        })
        .await;
        let result = folder(stream).await;
        assert_eq!(result, "Hello World");
    }

    #[tokio::test]
    async fn fold_single_element() {
        let stream: Pin<Box<dyn futures::Stream<Item = i32>>> = Box::pin(stream::iter(vec![42]));
        let folder = fold(|acc: i32, x| acc + x).await;
        let result = folder(stream).await;
        assert_eq!(result, 42);
    }

    // ================================
    // passthrough Combinator Tests
    // ================================

    #[tokio::test]
    async fn passthrough_inspects_all_items() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let stream: Pin<Box<dyn futures::Stream<Item = i32> + Send>> =
            Box::pin(stream::iter(vec![1, 2, 3, 4, 5]));
        let inspected = passthrough(move |_: &i32| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        })(stream);

        let result: Vec<i32> = inspected.collect().await;
        assert_eq!(result, vec![1, 2, 3, 4, 5]);
        assert_eq!(counter.load(Ordering::SeqCst), 5);
        println!(
            "passthrough_inspects_all_items: inspected {} items",
            counter.load(Ordering::SeqCst)
        );
    }

    #[tokio::test]
    async fn passthrough_does_not_modify_items() {
        let stream: Pin<Box<dyn futures::Stream<Item = i32> + Send>> =
            Box::pin(stream::iter(vec![1, 2, 3]));
        let inspected = passthrough(|_: &i32| {
            // Side effect only, no modification
        })(stream);

        let result: Vec<i32> = inspected.collect().await;
        assert_eq!(result, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn passthrough_empty_stream() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let stream: Pin<Box<dyn futures::Stream<Item = i32> + Send>> =
            Box::pin(stream::iter(vec![]));
        let inspected = passthrough(move |_: &i32| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        })(stream);

        let result: Vec<i32> = inspected.collect().await;
        assert!(result.is_empty());
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    // ================================
    // ContentBlockBuilder Tests
    // ================================

    #[test]
    fn content_block_builder_empty_builds_none() {
        let builder = ContentBlockBuilder::Empty;
        assert!(builder.build().is_none());
    }

    #[test]
    fn content_block_builder_text_accumulates_deltas() {
        let mut builder = ContentBlockBuilder::Text {
            text: String::new(),
            citations: None,
            cache_control: None,
        };

        builder.apply_delta(ContentBlockDelta::TextDelta(TextDelta::new(
            "Hello".to_string(),
        )));
        builder.apply_delta(ContentBlockDelta::TextDelta(TextDelta::new(
            " ".to_string(),
        )));
        builder.apply_delta(ContentBlockDelta::TextDelta(TextDelta::new(
            "World".to_string(),
        )));

        let block = builder.build().unwrap();
        match block {
            ContentBlock::Text(text_block) => {
                assert_eq!(text_block.text, "Hello World");
            }
            _ => panic!("Expected Text block"),
        }
    }

    #[test]
    fn content_block_builder_tool_use_accumulates_json() {
        let mut builder = ContentBlockBuilder::ToolUse {
            id: "tool_123".to_string(),
            name: "search".to_string(),
            input_json: String::new(),
            cache_control: None,
        };

        builder.apply_delta(ContentBlockDelta::InputJsonDelta(InputJsonDelta::new(
            r#"{"query":"#.to_string(),
        )));
        builder.apply_delta(ContentBlockDelta::InputJsonDelta(InputJsonDelta::new(
            r#""test"}"#.to_string(),
        )));

        let block = builder.build().unwrap();
        match block {
            ContentBlock::ToolUse(tool_use) => {
                assert_eq!(tool_use.id, "tool_123");
                assert_eq!(tool_use.name, "search");
                assert_eq!(tool_use.input, serde_json::json!({"query": "test"}));
            }
            _ => panic!("Expected ToolUse block"),
        }
    }

    #[test]
    fn content_block_builder_tool_use_invalid_json_becomes_null() {
        let mut builder = ContentBlockBuilder::ToolUse {
            id: "tool_123".to_string(),
            name: "search".to_string(),
            input_json: String::new(),
            cache_control: None,
        };

        builder.apply_delta(ContentBlockDelta::InputJsonDelta(InputJsonDelta::new(
            "not valid json".to_string(),
        )));

        let block = builder.build().unwrap();
        match block {
            ContentBlock::ToolUse(tool_use) => {
                assert_eq!(tool_use.input, serde_json::Value::Null);
            }
            _ => panic!("Expected ToolUse block"),
        }
    }

    #[test]
    fn content_block_builder_thinking_accumulates_thinking_and_signature() {
        let mut builder = ContentBlockBuilder::Thinking {
            thinking: String::new(),
            signature: String::new(),
        };

        builder.apply_delta(ContentBlockDelta::ThinkingDelta(ThinkingDelta::new(
            "Let me ".to_string(),
        )));
        builder.apply_delta(ContentBlockDelta::ThinkingDelta(ThinkingDelta::new(
            "think...".to_string(),
        )));
        builder.apply_delta(ContentBlockDelta::SignatureDelta(SignatureDelta::new(
            "sig123".to_string(),
        )));

        let block = builder.build().unwrap();
        match block {
            ContentBlock::Thinking(thinking) => {
                assert_eq!(thinking.thinking, "Let me think...");
                assert_eq!(thinking.signature, "sig123");
            }
            _ => panic!("Expected Thinking block"),
        }
    }

    #[test]
    fn content_block_builder_from_content_block_text() {
        let text_block = TextBlock::new("Initial text");
        let builder = ContentBlockBuilder::from_content_block(ContentBlock::Text(text_block));

        match builder {
            ContentBlockBuilder::Text { text, .. } => {
                assert_eq!(text, "Initial text");
            }
            _ => panic!("Expected Text builder"),
        }
    }

    #[test]
    fn content_block_builder_from_content_block_tool_use() {
        let tool_use = ToolUseBlock::new("id", "name", serde_json::json!({"key": "value"}));
        let builder = ContentBlockBuilder::from_content_block(ContentBlock::ToolUse(tool_use));

        match builder {
            ContentBlockBuilder::ToolUse {
                id,
                name,
                input_json,
                ..
            } => {
                assert_eq!(id, "id");
                assert_eq!(name, "name");
                // Input JSON starts empty for streaming accumulation
                assert_eq!(input_json, "");
            }
            _ => panic!("Expected ToolUse builder"),
        }
    }

    #[test]
    fn content_block_builder_mismatched_delta_ignored() {
        let mut builder = ContentBlockBuilder::Text {
            text: "Hello".to_string(),
            citations: None,
            cache_control: None,
        };

        // Applying an InputJsonDelta to a Text builder should be ignored
        builder.apply_delta(ContentBlockDelta::InputJsonDelta(InputJsonDelta::new(
            "ignored".to_string(),
        )));

        let block = builder.build().unwrap();
        match block {
            ContentBlock::Text(text_block) => {
                assert_eq!(text_block.text, "Hello");
            }
            _ => panic!("Expected Text block"),
        }
    }

    // ================================
    // AccumulatingStream Tests
    // ================================

    #[tokio::test]
    async fn accumulating_stream_simple_text_message() {
        let events = vec![
            Ok(make_message_start(make_test_message())),
            Ok(make_content_block_start_text(0, "")),
            Ok(make_text_delta(0, "Hello")),
            Ok(make_text_delta(0, " World")),
            Ok(make_content_block_stop(0)),
            Ok(make_message_delta(Some(StopReason::EndTurn), 10)),
            Ok(make_message_stop()),
        ];

        let stream: Pin<
            Box<dyn futures::Stream<Item = Result<MessageStreamEvent, crate::Error>> + Send>,
        > = Box::pin(stream::iter(events));

        let (mut acc_stream, rx) = AccumulatingStream::new(stream);

        // Drain the stream
        let mut event_count = 0;
        while let Some(result) = acc_stream.next().await {
            result.unwrap();
            event_count += 1;
        }
        assert_eq!(event_count, 7);

        // Get the accumulated message
        let message = rx.await.unwrap();
        assert_eq!(message.id, "msg_test");
        assert_eq!(message.content.len(), 1);
        match &message.content[0] {
            ContentBlock::Text(text_block) => {
                assert_eq!(text_block.text, "Hello World");
            }
            _ => panic!("Expected Text block"),
        }
        assert_eq!(message.stop_reason, Some(StopReason::EndTurn));
        println!(
            "accumulating_stream_simple_text_message: accumulated message with {} content blocks",
            message.content.len()
        );
    }

    #[tokio::test]
    async fn accumulating_stream_tool_use_message() {
        let events = vec![
            Ok(make_message_start(make_test_message())),
            Ok(make_content_block_start_tool_use(0, "tool_1", "search")),
            Ok(make_input_json_delta(0, r#"{"query":"#)),
            Ok(make_input_json_delta(0, r#""test"}"#)),
            Ok(make_content_block_stop(0)),
            Ok(make_message_delta(Some(StopReason::ToolUse), 15)),
            Ok(make_message_stop()),
        ];

        let stream: Pin<
            Box<dyn futures::Stream<Item = Result<MessageStreamEvent, crate::Error>> + Send>,
        > = Box::pin(stream::iter(events));

        let (mut acc_stream, rx) = AccumulatingStream::new(stream);

        // Drain the stream
        while acc_stream.next().await.is_some() {}

        let message = rx.await.unwrap();
        assert_eq!(message.content.len(), 1);
        match &message.content[0] {
            ContentBlock::ToolUse(tool_use) => {
                assert_eq!(tool_use.id, "tool_1");
                assert_eq!(tool_use.name, "search");
                assert_eq!(tool_use.input, serde_json::json!({"query": "test"}));
            }
            _ => panic!("Expected ToolUse block"),
        }
        assert_eq!(message.stop_reason, Some(StopReason::ToolUse));
    }

    #[tokio::test]
    async fn accumulating_stream_multiple_content_blocks() {
        let events = vec![
            Ok(make_message_start(make_test_message())),
            Ok(make_content_block_start_text(0, "")),
            Ok(make_text_delta(0, "Here's the search:")),
            Ok(make_content_block_stop(0)),
            Ok(make_content_block_start_tool_use(1, "tool_1", "search")),
            Ok(make_input_json_delta(1, r#"{"q":"x"}"#)),
            Ok(make_content_block_stop(1)),
            Ok(make_message_delta(Some(StopReason::ToolUse), 20)),
            Ok(make_message_stop()),
        ];

        let stream: Pin<
            Box<dyn futures::Stream<Item = Result<MessageStreamEvent, crate::Error>> + Send>,
        > = Box::pin(stream::iter(events));

        let (mut acc_stream, rx) = AccumulatingStream::new(stream);
        while acc_stream.next().await.is_some() {}

        let message = rx.await.unwrap();
        assert_eq!(message.content.len(), 2);
        assert!(message.content[0].is_text());
        assert!(message.content[1].is_tool_use());
    }

    #[tokio::test]
    async fn accumulating_stream_thinking_block() {
        let events = vec![
            Ok(make_message_start(make_test_message())),
            Ok(make_content_block_start_thinking(0)),
            Ok(make_thinking_delta(0, "Let me ")),
            Ok(make_thinking_delta(0, "analyze this...")),
            Ok(make_signature_delta(0, "sig_abc")),
            Ok(make_content_block_stop(0)),
            Ok(make_content_block_start_text(1, "")),
            Ok(make_text_delta(1, "Result")),
            Ok(make_content_block_stop(1)),
            Ok(make_message_delta(Some(StopReason::EndTurn), 30)),
            Ok(make_message_stop()),
        ];

        let stream: Pin<
            Box<dyn futures::Stream<Item = Result<MessageStreamEvent, crate::Error>> + Send>,
        > = Box::pin(stream::iter(events));

        let (mut acc_stream, rx) = AccumulatingStream::new(stream);
        while acc_stream.next().await.is_some() {}

        let message = rx.await.unwrap();
        assert_eq!(message.content.len(), 2);

        match &message.content[0] {
            ContentBlock::Thinking(thinking) => {
                assert_eq!(thinking.thinking, "Let me analyze this...");
                assert_eq!(thinking.signature, "sig_abc");
            }
            _ => panic!("Expected Thinking block"),
        }
    }

    #[tokio::test]
    async fn accumulating_stream_empty_message() {
        let events = vec![
            Ok(make_message_start(make_test_message())),
            Ok(make_message_delta(Some(StopReason::EndTurn), 0)),
            Ok(make_message_stop()),
        ];

        let stream: Pin<
            Box<dyn futures::Stream<Item = Result<MessageStreamEvent, crate::Error>> + Send>,
        > = Box::pin(stream::iter(events));

        let (mut acc_stream, rx) = AccumulatingStream::new(stream);
        while acc_stream.next().await.is_some() {}

        let message = rx.await.unwrap();
        assert!(message.content.is_empty());
    }

    #[tokio::test]
    async fn accumulating_stream_ping_events_ignored() {
        let events = vec![
            Ok(MessageStreamEvent::Ping),
            Ok(make_message_start(make_test_message())),
            Ok(MessageStreamEvent::Ping),
            Ok(make_content_block_start_text(0, "")),
            Ok(make_text_delta(0, "Test")),
            Ok(MessageStreamEvent::Ping),
            Ok(make_content_block_stop(0)),
            Ok(make_message_delta(Some(StopReason::EndTurn), 5)),
            Ok(make_message_stop()),
        ];

        let stream: Pin<
            Box<dyn futures::Stream<Item = Result<MessageStreamEvent, crate::Error>> + Send>,
        > = Box::pin(stream::iter(events));

        let (mut acc_stream, rx) = AccumulatingStream::new(stream);
        while acc_stream.next().await.is_some() {}

        let message = rx.await.unwrap();
        match &message.content[0] {
            ContentBlock::Text(text_block) => {
                assert_eq!(text_block.text, "Test");
            }
            _ => panic!("Expected Text block"),
        }
    }

    // ================================
    // extract_tool_uses Tests
    // ================================

    #[test]
    fn extract_tool_uses_empty_message() {
        let message = make_test_message();
        let tool_uses = extract_tool_uses(&message);
        assert!(tool_uses.is_empty());
    }

    #[test]
    fn extract_tool_uses_no_tools() {
        let mut message = make_test_message();
        message.content = vec![ContentBlock::Text(TextBlock::new("Just text"))];
        let tool_uses = extract_tool_uses(&message);
        assert!(tool_uses.is_empty());
    }

    #[test]
    fn extract_tool_uses_single_tool() {
        let mut message = make_test_message();
        message.content = vec![ContentBlock::ToolUse(ToolUseBlock::new(
            "tool_1",
            "search",
            serde_json::json!({"q": "test"}),
        ))];

        let tool_uses = extract_tool_uses(&message);
        assert_eq!(tool_uses.len(), 1);
        assert_eq!(tool_uses[0].id, "tool_1");
        assert_eq!(tool_uses[0].name, "search");
    }

    #[test]
    fn extract_tool_uses_multiple_tools() {
        let mut message = make_test_message();
        message.content = vec![
            ContentBlock::Text(TextBlock::new("Let me search")),
            ContentBlock::ToolUse(ToolUseBlock::new("tool_1", "search", serde_json::json!({}))),
            ContentBlock::Text(TextBlock::new("And calculate")),
            ContentBlock::ToolUse(ToolUseBlock::new(
                "tool_2",
                "calculator",
                serde_json::json!({}),
            )),
        ];

        let tool_uses = extract_tool_uses(&message);
        assert_eq!(tool_uses.len(), 2);
        assert_eq!(tool_uses[0].id, "tool_1");
        assert_eq!(tool_uses[1].id, "tool_2");
    }

    #[test]
    fn extract_tool_uses_preserves_order() {
        let mut message = make_test_message();
        message.content = vec![
            ContentBlock::ToolUse(ToolUseBlock::new("tool_a", "first", serde_json::json!({}))),
            ContentBlock::ToolUse(ToolUseBlock::new("tool_b", "second", serde_json::json!({}))),
            ContentBlock::ToolUse(ToolUseBlock::new("tool_c", "third", serde_json::json!({}))),
        ];

        let tool_uses = extract_tool_uses(&message);
        assert_eq!(tool_uses[0].name, "first");
        assert_eq!(tool_uses[1].name, "second");
        assert_eq!(tool_uses[2].name, "third");
    }

    // ================================
    // is_tool_use Tests
    // ================================

    #[test]
    fn is_tool_use_with_tool_use_stop_reason() {
        let message = make_test_message().with_stop_reason(StopReason::ToolUse);
        assert!(is_tool_use(&message));
    }

    #[test]
    fn is_tool_use_with_end_turn_stop_reason() {
        let message = make_test_message().with_stop_reason(StopReason::EndTurn);
        assert!(!is_tool_use(&message));
    }

    #[test]
    fn is_tool_use_with_max_tokens_stop_reason() {
        let message = make_test_message().with_stop_reason(StopReason::MaxTokens);
        assert!(!is_tool_use(&message));
    }

    #[test]
    fn is_tool_use_with_no_stop_reason() {
        let message = make_test_message();
        assert!(message.stop_reason.is_none());
        assert!(!is_tool_use(&message));
    }

    // ================================
    // tool_results_for_message Tests
    // ================================

    #[test]
    fn tool_results_for_message_empty() {
        let message = make_test_message();
        let results = tool_results_for_message(&message, |_| Ok("success".to_string()));
        assert!(results.is_empty());
    }

    #[test]
    fn tool_results_for_message_single_success() {
        let mut message = make_test_message();
        message.content = vec![ContentBlock::ToolUse(ToolUseBlock::new(
            "tool_1",
            "search",
            serde_json::json!({}),
        ))];

        let results = tool_results_for_message(&message, |tool_use| {
            Ok(format!("Result for {}", tool_use.name))
        });

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].tool_use_id, "tool_1");
        assert_eq!(results[0].is_error, Some(false));
        match &results[0].content {
            Some(crate::ToolResultBlockContent::String(s)) => {
                assert_eq!(s, "Result for search");
            }
            _ => panic!("Expected String content"),
        }
    }

    #[test]
    fn tool_results_for_message_single_error() {
        let mut message = make_test_message();
        message.content = vec![ContentBlock::ToolUse(ToolUseBlock::new(
            "tool_1",
            "failing_tool",
            serde_json::json!({}),
        ))];

        let results = tool_results_for_message(&message, |_| Err("Tool failed".to_string()));

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].is_error, Some(true));
        match &results[0].content {
            Some(crate::ToolResultBlockContent::String(s)) => {
                assert_eq!(s, "Tool failed");
            }
            _ => panic!("Expected String content"),
        }
    }

    #[test]
    fn tool_results_for_message_multiple_mixed() {
        let mut message = make_test_message();
        message.content = vec![
            ContentBlock::ToolUse(ToolUseBlock::new(
                "tool_1",
                "success",
                serde_json::json!({}),
            )),
            ContentBlock::ToolUse(ToolUseBlock::new(
                "tool_2",
                "failure",
                serde_json::json!({}),
            )),
            ContentBlock::ToolUse(ToolUseBlock::new(
                "tool_3",
                "success",
                serde_json::json!({}),
            )),
        ];

        let results = tool_results_for_message(&message, |tool_use| {
            if tool_use.name == "failure" {
                Err("Error!".to_string())
            } else {
                Ok("OK".to_string())
            }
        });

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].is_error, Some(false));
        assert_eq!(results[1].is_error, Some(true));
        assert_eq!(results[2].is_error, Some(false));
    }

    #[test]
    fn tool_results_for_message_preserves_tool_use_id() {
        let mut message = make_test_message();
        message.content = vec![
            ContentBlock::ToolUse(ToolUseBlock::new("id_alpha", "t1", serde_json::json!({}))),
            ContentBlock::ToolUse(ToolUseBlock::new("id_beta", "t2", serde_json::json!({}))),
        ];

        let results = tool_results_for_message(&message, |_| Ok("done".to_string()));

        assert_eq!(results[0].tool_use_id, "id_alpha");
        assert_eq!(results[1].tool_use_id, "id_beta");
    }

    // ================================
    // to_message Combinator Tests
    // ================================

    #[tokio::test]
    async fn to_message_simple_text() {
        use super::to_message;

        let events = vec![
            Ok(make_message_start(make_test_message())),
            Ok(make_content_block_start_text(0, "")),
            Ok(make_text_delta(0, "Hello")),
            Ok(make_content_block_stop(0)),
            Ok(make_message_delta(Some(StopReason::EndTurn), 5)),
            Ok(make_message_stop()),
        ];

        let stream: Pin<
            Box<dyn futures::Stream<Item = Result<MessageStreamEvent, crate::Error>> + Send>,
        > = Box::pin(stream::iter(events));

        let message = to_message()(stream).await.unwrap();

        assert_eq!(message.id, "msg_test");
        assert_eq!(message.content.len(), 1);
        match &message.content[0] {
            ContentBlock::Text(text_block) => {
                assert_eq!(text_block.text, "Hello");
            }
            _ => panic!("Expected Text block"),
        }
    }

    #[tokio::test]
    async fn to_message_propagates_errors() {
        use super::to_message;

        let events: Vec<Result<MessageStreamEvent, crate::Error>> = vec![
            Ok(make_message_start(make_test_message())),
            Err(crate::Error::Api {
                status_code: 500,
                error_type: Some("test".to_string()),
                message: "Test error".to_string(),
                request_id: None,
            }),
        ];

        let stream: Pin<
            Box<dyn futures::Stream<Item = Result<MessageStreamEvent, crate::Error>> + Send>,
        > = Box::pin(stream::iter(events));

        let result = to_message()(stream).await;
        assert!(result.is_err());
    }

    // ================================
    // tool_results_for_message_async Tests
    // ================================

    #[tokio::test]
    async fn tool_results_for_message_async_concurrent_execution() {
        use super::tool_results_for_message_async;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let mut message = make_test_message();
        message.content = vec![
            ContentBlock::ToolUse(ToolUseBlock::new("tool_1", "t1", serde_json::json!({}))),
            ContentBlock::ToolUse(ToolUseBlock::new("tool_2", "t2", serde_json::json!({}))),
            ContentBlock::ToolUse(ToolUseBlock::new("tool_3", "t3", serde_json::json!({}))),
        ];

        let call_count = Arc::new(AtomicUsize::new(0));

        let results = tool_results_for_message_async(&message, |tool_use| {
            let call_count = call_count.clone();
            let name = tool_use.name.clone();
            async move {
                call_count.fetch_add(1, Ordering::SeqCst);
                Ok(format!("result_{}", name))
            }
        })
        .await;

        assert_eq!(results.len(), 3);
        assert_eq!(call_count.load(Ordering::SeqCst), 3);

        // Results should be in order regardless of completion order
        assert_eq!(results[0].tool_use_id, "tool_1");
        assert_eq!(results[1].tool_use_id, "tool_2");
        assert_eq!(results[2].tool_use_id, "tool_3");
        println!(
            "tool_results_for_message_async_concurrent_execution: executed {} tools",
            call_count.load(Ordering::SeqCst)
        );
    }
}
