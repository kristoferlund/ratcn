match ratcn.handle_event(event, &state) {
    EventResult::Emit(msg) => state.update(msg),
    EventResult::Consumed => {}
    EventResult::Ignored => {}
}
