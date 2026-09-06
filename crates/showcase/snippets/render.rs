ratcn.render(frame, area, &state, &theme, |ctx| {
    let button = Button::new("Hello")
        .on_press(|| Msg::Hello);
    ctx.component("hello", button, area);
})
