//! Integration tests for the screen layer stack.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use engine_ui::{Layer, ScreenId, ScreenStack, UiError};
use proptest::prelude::*;

#[test]
fn toast_never_focused() {
    let mut stack = ScreenStack::new();
    stack.push(Layer::Toast, ScreenId(1));
    assert_eq!(stack.focused(), None);
    stack.push(Layer::Menu, ScreenId(2));
    assert_eq!(stack.focused(), Some((Layer::Menu, ScreenId(2))));
}

#[test]
fn back_pops_modal_before_menu() {
    let mut stack = ScreenStack::new();
    stack.push(Layer::Menu, ScreenId(1));
    stack.push(Layer::Modal, ScreenId(2));
    assert_eq!(stack.pop_top().unwrap(), (Layer::Modal, ScreenId(2)));
    assert_eq!(stack.pop_top().unwrap(), (Layer::Menu, ScreenId(1)));
}

#[test]
fn pop_empty_errors() {
    let mut stack = ScreenStack::new();
    assert_eq!(stack.pop_top().unwrap_err(), UiError::EmptyStack);
}

#[derive(Debug, Clone)]
enum Op {
    Push(Layer, u32),
    Pop,
    Clear(Layer),
}

proptest! {
    #[test]
    fn focused_matches_reference(ops in prop::collection::vec(any_op(), 0..80)) {
        let mut stack = ScreenStack::new();
        let mut model: [Vec<u32>; Layer::ALL.len()] =
            std::array::from_fn(|_| Vec::new());
        for op in ops {
            match op {
                Op::Push(layer, id) => {
                    stack.push(layer, ScreenId(id));
                    if let Some(slice) = model_slot_mut(&mut model, layer) {
                        slice.push(id);
                    }
                }
                Op::Pop => {
                    if stack.pop_top().is_ok() {
                        pop_model(&mut model);
                    }
                }
                Op::Clear(layer) => {
                    stack.clear(layer);
                    if let Some(slice) = model_slot_mut(&mut model, layer) {
                        slice.clear();
                    }
                }
            }
            assert_eq!(stack.focused(), model_focused(&model));
        }
    }
}

fn any_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (any_layer(), any::<u32>()).prop_map(|(l, id)| Op::Push(l, id)),
        Just(Op::Pop),
        any_layer().prop_map(Op::Clear),
    ]
}

fn any_layer() -> impl Strategy<Value = Layer> {
    prop_oneof![
        Just(Layer::Game),
        Just(Layer::GameMenu),
        Just(Layer::Menu),
        Just(Layer::Modal),
        Just(Layer::Toast),
    ]
}

fn model_slot_mut(model: &mut [Vec<u32>], layer: Layer) -> Option<&mut Vec<u32>> {
    Layer::ALL
        .iter()
        .zip(model.iter_mut())
        .find_map(|(slot_layer, stack)| (*slot_layer == layer).then_some(stack))
}

fn pop_model(model: &mut [Vec<u32>]) {
    for stack in model.iter_mut().rev() {
        if stack.pop().is_some() {
            return;
        }
    }
}

fn model_focused(model: &[Vec<u32>]) -> Option<(Layer, ScreenId)> {
    for (layer, stack) in Layer::ALL.iter().rev().zip(model.iter().rev()) {
        if !layer.accepts_focus() {
            continue;
        }
        if let Some(&id) = stack.last() {
            return Some((*layer, ScreenId(id)));
        }
    }
    None
}
