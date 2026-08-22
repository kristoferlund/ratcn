//! The contract [`DeclareCtx::viewport`] holds to, independent of any
//! component that opens one.

use ratatui::{
    style::Style,
    widgets::{Paragraph, StatefulWidget, Widget},
};

use super::*;

#[derive(Default)]
struct State;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Msg {
    Pressed,
    Dragged(CellOffset),
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> &str {
    panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .unwrap_or_default()
}

#[test]
fn viewport_cell_limit_accepts_the_boundary_and_rejects_one_row_more() {
    let mut boundary = Driver::<State, Msg>::new(1, 1);
    boundary.render(&State, |ctx| {
        ctx.viewport(Rect::new(0, 0, 512, 1), 512, 0, |_| {});
    });

    let mut over = Driver::<State, Msg>::new(1, 1);
    let result = catch_unwind(AssertUnwindSafe(|| {
        over.render(&State, |ctx| {
            ctx.viewport(Rect::new(0, 0, 512, 1), 513, 0, |_| {});
        });
    }));
    let panic = result.expect_err("a viewport above the cell limit must panic");
    assert!(
        panic_message(panic.as_ref()).contains("maximum is 262144"),
        "{}",
        panic_message(panic.as_ref())
    );
}

/// `content_height` is app-computed over data that changes, so an
/// allocation one row too tall paints the rows that fit and the frame
/// carries on.
#[test]
fn paint_outside_the_logical_content_is_clipped() {
    let mut driver = Driver::<State, Msg>::new(4, 2);
    driver.render(&State, |ctx| {
        ctx.viewport(Rect::new(0, 0, 2, 1), 2, 0, |ctx| {
            ctx.paint_widget(Paragraph::new("wide"), Rect::new(0, 0, 4, 1));
        });
    });

    assert_eq!(driver.row(0), "wi  ");
}

/// Content shorter than the rectangle showing it leaves the rows past its
/// end to whatever is painted beneath.
#[test]
fn paint_below_the_logical_content_is_clipped() {
    let mut driver = Driver::<State, Msg>::new(4, 3);
    driver.render(&State, |ctx| {
        ctx.paint_widget(Paragraph::new("keep"), Rect::new(0, 2, 4, 1));
        ctx.viewport(Rect::new(0, 0, 4, 3), 2, 0, |ctx| {
            ctx.paint_widget(Paragraph::new("over"), Rect::new(0, 2, 4, 1));
        });
    });

    assert_eq!(driver.row(2), "keep");
}

/// A paint inside a viewport starts from what is already on the surface
/// beneath it, so the cells its widget leaves alone keep what was there.
#[test]
fn paint_inside_a_viewport_keeps_the_cells_its_widget_leaves_alone() {
    let mut driver = Driver::<State, Msg>::new(4, 1);
    driver.render(&State, |ctx| {
        ctx.paint_widget(Paragraph::new("keep"), Rect::new(0, 0, 4, 1));
        ctx.viewport(Rect::new(0, 0, 4, 1), 1, 0, |ctx| {
            ctx.paint_widget(Paragraph::new("X"), Rect::new(0, 0, 4, 1));
        });
    });

    assert_eq!(driver.row(0), "Xeep");
}

#[derive(Debug, Clone, Copy)]
enum CaughtViewportFailure {
    Widget,
    StatefulWidget,
    WithBuffer,
}

struct Leaf;

impl Component<State, Msg> for Leaf {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, State, Msg>) {}

    fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, State>) {
        ctx.widget(Paragraph::new("stable"), ctx.area());
    }

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &State,
        _ctx: &mut EventCtx<'_>,
    ) -> EventResult<Msg> {
        match event {
            Event::Mouse(mouse) if mouse.kind == MouseKind::Click(MouseButton::Left) => {
                EventResult::Emit(Msg::Pressed)
            }
            _ => EventResult::Ignored,
        }
    }

    fn scope_options(&self) -> ScopeOptions {
        ScopeOptions::default().focusable(true)
    }
}

/// Every paint inside a viewport lays out in one buffer the pass reuses, so
/// what a paint reads where it has written nothing is blank. The seeding copy
/// cannot answer for the logical rows the viewport does not show: they never
/// reach the frame, so only blanking the buffer keeps one paint's leftovers
/// out of the next one's layout.
#[test]
fn a_paint_inside_a_viewport_reads_blanks_where_it_has_not_written() {
    let seen = Rc::new(RefCell::new(String::new()));
    let read = Rc::clone(&seen);
    let mut driver = Driver::<State, Msg>::new(4, 1);
    driver.render(&State, move |ctx| {
        ctx.viewport(Rect::new(0, 0, 4, 1), 4, 0, move |ctx| {
            ctx.paint(|ctx| {
                ctx.with_buffer(|buffer| {
                    buffer[(0, 3)].set_symbol("X");
                });
            });
            ctx.paint(move |ctx| {
                ctx.with_buffer(|buffer| {
                    read.borrow_mut().push_str(buffer[(0, 3)].symbol());
                });
            });
        });
    });

    assert_eq!(*seen.borrow(), " ");
}

/// Paint that escaped a viewport's clip addresses the surface it lands on,
/// read in logical coordinates: the frame shifted down by the scroll offset.
/// A popup declared inside a viewport therefore reaches every row of its own
/// canvas, including the rows the viewport itself does not show — the
/// viewport's content rectangle is the allocation of the content it clips,
/// and says nothing about what left the clip.
#[test]
fn escaped_paint_is_allocated_the_whole_surface_in_logical_coordinates() {
    let mut driver = Driver::<State, Msg>::new(10, 4);
    driver.render(&State, |ctx| {
        ctx.viewport(Rect::new(0, 0, 6, 2), 8, 3, |ctx| {
            ctx.popup(
                "pop",
                Rect::new(0, 3, 10, 4),
                PopupOptions::default(),
                |ctx| {
                    ctx.defer_paint(|ctx| {
                        let area = ctx.area();
                        ctx.with_buffer(move |buffer| {
                            buffer.set_string(area.x, area.y, "T", Style::default());
                            buffer.set_string(area.x, area.bottom() - 1, "B", Style::default());
                        });
                    });
                },
            );
        });
    });

    // Logical row 3 is the frame's first row at offset 3, and logical row 6
    // its last.
    assert_eq!(driver.row(0), "T         ");
    assert_eq!(driver.row(3), "B         ");
}

struct PanicWidget;

impl Widget for PanicWidget {
    fn render(self, _area: Rect, buf: &mut Buffer) {
        buf[(0, 0)].set_symbol("X");
        panic!("widget paint failed");
    }
}

impl StatefulWidget for PanicWidget {
    type State = ();

    fn render(self, area: Rect, buf: &mut Buffer, _state: &mut Self::State) {
        Widget::render(self, area, buf);
    }
}

/// A paint panic a component catches is the component's own business: the
/// pass finishes and commits. What the panicking paint wrote still never
/// reaches the frame, because a paint inside a viewport lays out in a
/// scratch buffer that is copied back only once it returns.
#[test]
fn a_caught_viewport_paint_panic_writes_nothing_and_commits() {
    for failure in [
        CaughtViewportFailure::Widget,
        CaughtViewportFailure::StatefulWidget,
        CaughtViewportFailure::WithBuffer,
    ] {
        let mut driver = Driver::<State, Msg>::new(10, 3);
        driver.render(&State, |ctx| {
            ctx.component("stable", Leaf, Rect::new(0, 0, 6, 1));
        });

        driver
            .terminal
            .draw(|frame| {
                // The backend clears between draws, so the frame this pass
                // must leave alone is painted back first.
                frame.render_widget(Paragraph::new("stable"), Rect::new(0, 0, 6, 1));
                driver
                    .ratcn
                    .render(frame, &State, &Theme::default_dark(), move |ctx| {
                        ctx.viewport(Rect::new(0, 0, 2, 1), 2, 0, move |ctx| {
                            ctx.paint(move |ctx| {
                                let caught = catch_unwind(AssertUnwindSafe(|| {
                                    let area = Rect::new(0, 0, 2, 1);
                                    match failure {
                                        CaughtViewportFailure::Widget => {
                                            ctx.widget(PanicWidget, area);
                                        }
                                        CaughtViewportFailure::StatefulWidget => {
                                            ctx.stateful_widget(PanicWidget, area, &mut ());
                                        }
                                        CaughtViewportFailure::WithBuffer => {
                                            ctx.with_buffer(|buffer| {
                                                buffer[(0, 0)].set_symbol("X");
                                                panic!("component paint failed");
                                            });
                                        }
                                    }
                                }));
                                assert!(caught.is_err());
                            });
                        });
                    });
            })
            .expect("draw");

        assert_eq!(
            driver.row(0),
            "stable    ",
            "{failure:?} let an unrecorded viewport write reach the frame"
        );
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 1, 0), &State),
            EventResult::Ignored,
            "{failure:?} did not commit the pass that caught the panic"
        );
    }
}

/// A layer canvas holds the same guarantee: a write the panic left
/// unrecorded composites nowhere.
#[test]
fn a_caught_layer_paint_panic_composites_nothing_and_commits() {
    let mut driver = Driver::<State, Msg>::new(10, 3);
    driver.render(&State, |ctx| {
        ctx.component("stable", Leaf, Rect::new(0, 0, 6, 1));
    });

    driver
        .terminal
        .draw(|frame| {
            frame.render_widget(Paragraph::new("stable"), Rect::new(0, 0, 6, 1));
            driver
                .ratcn
                .render(frame, &State, &Theme::default_dark(), |ctx| {
                    ctx.popup(
                        "popup",
                        Rect::new(0, 0, 4, 1),
                        PopupOptions::default(),
                        |ctx| {
                            ctx.paint(|ctx| {
                                let caught = catch_unwind(AssertUnwindSafe(|| {
                                    ctx.widget(PanicWidget, Rect::new(0, 0, 4, 1));
                                }));
                                assert!(caught.is_err());
                            });
                        },
                    );
                });
        })
        .expect("draw");

    assert_eq!(
        driver.row(0),
        "stable    ",
        "an unrecorded layer write reached the frame"
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Click(MouseButton::Left), 1, 0), &State),
        EventResult::Consumed,
        "the popup layer did not commit the pass that caught the panic"
    );
}

struct DragProbe;

impl Component<State, Msg> for DragProbe {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, State, Msg>) {}

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &State,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<Msg> {
        let Event::Mouse(mouse) = event else {
            return EventResult::Ignored;
        };
        match ctx.drag(mouse, DragOptions::default()) {
            DragPhase::Down | DragPhase::Ended { .. } => EventResult::Consumed,
            DragPhase::Moved { offset, .. } => EventResult::Emit(Msg::Dragged(offset)),
            DragPhase::Ignored => EventResult::Ignored,
        }
    }
}

fn render_drag_viewport(driver: &mut Driver<State, Msg>, offset: u16) {
    driver.render(&State, |ctx| {
        ctx.viewport(Rect::new(0, 0, 7, 3), 8, offset, |ctx| {
            ctx.component("drag", DragProbe, Rect::new(0, 1, 7, 1));
        });
    });
}

#[test]
fn a_captured_drag_keeps_routing_after_leaving_the_viewport() {
    let mut driver = Driver::<State, Msg>::new(8, 5);
    render_drag_viewport(&mut driver, 0);

    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &State),
        EventResult::Consumed
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 1, 4), &State),
        EventResult::Emit(Msg::Dragged(CellOffset::new(0, 3))),
        "capture, not viewport hit-testing, owns the drag"
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Up(MouseButton::Left), 1, 4), &State),
        EventResult::Consumed
    );
}

/// The drag anchor is screen-absolute, so scrolling under a held pointer
/// leaves the travel it measures alone.
#[test]
fn scrolling_during_a_captured_drag_leaves_its_offset_measuring_travel() {
    let mut driver = Driver::<State, Msg>::new(8, 5);
    render_drag_viewport(&mut driver, 0);
    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &State),
        EventResult::Consumed
    );

    render_drag_viewport(&mut driver, 2);
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 1, 2), &State),
        EventResult::Emit(Msg::Dragged(CellOffset::new(0, 1)))
    );
}

#[test]
fn a_captured_drag_reports_travel_at_the_coordinate_limit() {
    let mut driver = Driver::<State, Msg>::new(2, 3);
    driver.render(&State, |ctx| {
        ctx.viewport(Rect::new(0, 0, 1, 3), u16::MAX, u16::MAX, |ctx| {
            ctx.component("drag", DragProbe, Rect::new(0, u16::MAX - 3, 1, 1));
        });
    });

    assert_eq!(
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 0, 0), &State),
        EventResult::Consumed
    );
    assert_eq!(
        driver.event(mouse(MouseKind::Moved, 0, 4), &State),
        EventResult::Emit(Msg::Dragged(CellOffset::new(0, 4)))
    );
}

#[test]
fn deferred_paint_inside_a_viewport_projects_onto_the_frame() {
    let mut driver = Driver::<State, Msg>::new(2, 6);
    driver.render(&State, |ctx| {
        ctx.viewport(Rect::new(0, 0, 2, 3), u16::MAX, u16::MAX, |ctx| {
            ctx.defer_paint(|ctx| {
                ctx.with_buffer(|buffer| {
                    buffer.set_string(0, u16::MAX - 3, "D", Style::default());
                });
            });
        });
    });

    assert_eq!(&driver.row(0)[..1], "D");
}

/// A viewport's clip travels with its content's paint: rows declared
/// below the ones on screen land nowhere, leaving what is painted beneath
/// the scroll area alone.
#[test]
fn content_below_the_visible_rows_leaves_the_frame_beneath_it_alone() {
    let mut driver = Driver::<State, Msg>::new(4, 4);
    driver.render(&State, |ctx| {
        ctx.paint_widget(Paragraph::new("keep"), Rect::new(0, 2, 4, 1));
        ctx.viewport(Rect::new(0, 0, 4, 2), 4, 0, |ctx| {
            ctx.paint_widget(Paragraph::new("in"), Rect::new(0, 0, 2, 1));
            ctx.paint_widget(Paragraph::new("out"), Rect::new(0, 2, 3, 1));
        });
    });

    assert_eq!(driver.row(0), "in  ");
    assert_eq!(
        driver.row(2),
        "keep",
        "content past the viewport rows bled out"
    );
}

/// A free-form paint inside a viewport covers the whole logical content,
/// so it may address rows the viewport is not showing.
#[test]
fn with_buffer_inside_a_viewport_covers_the_whole_logical_content() {
    let mut driver = Driver::<State, Msg>::new(4, 2);
    driver.render(&State, |ctx| {
        ctx.viewport(Rect::new(0, 0, 4, 2), 6, 4, |ctx| {
            ctx.paint(|ctx| {
                ctx.with_buffer(|buffer| {
                    buffer.set_string(0, 0, "hi", Style::default());
                    buffer.set_string(0, 4, "ok", Style::default());
                });
            });
        });
    });

    assert_eq!(driver.row(0), "ok  ");
}

/// A layer opened inside a viewport escaped that clip, and paints where
/// the offset puts it however far past the viewport's own rows that is.
#[test]
fn a_popup_declared_inside_a_viewport_paints_past_the_viewport_rows() {
    let mut driver = Driver::<State, Msg>::new(4, 4);
    driver.render(&State, |ctx| {
        ctx.viewport(Rect::new(0, 0, 4, 2), 4, 0, |ctx| {
            ctx.popup(
                "pop",
                Rect::new(0, 3, 3, 1),
                PopupOptions::default(),
                |ctx| {
                    ctx.paint_widget(Paragraph::new("pop"), Rect::new(0, 3, 3, 1));
                },
            );
        });
    });

    assert_eq!(driver.row(3), "pop ");
}

/// Paint inside a viewport keeps the frame's order: a later declaration
/// covers an earlier one, and every layer covers both.
#[test]
fn paint_inside_a_viewport_keeps_declaration_order_under_its_layers() {
    let mut driver = Driver::<State, Msg>::new(4, 2);
    driver.render(&State, |ctx| {
        ctx.viewport(Rect::new(0, 0, 4, 2), 2, 0, |ctx| {
            ctx.paint_widget(Paragraph::new("aaaa"), Rect::new(0, 0, 4, 1));
            ctx.paint_widget(Paragraph::new("bb"), Rect::new(0, 0, 2, 1));
            ctx.popup(
                "pop",
                Rect::new(0, 1, 2, 1),
                PopupOptions::default(),
                |ctx| {
                    ctx.paint_widget(Paragraph::new("PP"), Rect::new(0, 1, 2, 1));
                },
            );
            ctx.paint_widget(Paragraph::new("cccc"), Rect::new(0, 1, 4, 1));
        });
    });

    assert_eq!(
        driver.row(0),
        "bbaa",
        "a later declaration paints over an earlier"
    );
    assert_eq!(
        driver.row(1),
        "PPcc",
        "a layer paints over the content around it"
    );
}

/// A modal escapes the viewport it was declared in: the scroll is undone
/// once over its area, and a row carried above the top edge is held there.
/// A modal opened from content scrolled out of sight is on screen, exclusive
/// and visible together.
#[test]
fn a_modal_declared_from_a_scrolled_off_row_opens_on_the_screen() {
    let mut driver = Driver::<State, Msg>::new(6, 3);
    driver.render(&State, |ctx| {
        ctx.viewport(Rect::new(0, 0, 6, 3), 20, 17, |ctx| {
            ctx.modal_scope(
                "dialog",
                Rect::new(0, 1, 6, 1),
                ScopeOptions::default(),
                |ctx| {
                    let area = ctx.area();
                    ctx.paint_widget(Paragraph::new("MODAL"), area);
                },
            );
        });
    });

    assert_eq!(driver.row(0), "MODAL ");
}

/// A modal leaves its viewport only while it is being declared. What the same
/// declaration says afterwards is projected by that viewport as before, which
/// is why the offset is put back rather than dropped.
#[test]
fn content_declared_after_a_modal_is_still_projected_by_its_viewport() {
    let mut driver = Driver::<State, Msg>::new(4, 3);
    driver.render(&State, |ctx| {
        ctx.viewport(Rect::new(0, 0, 4, 2), 4, 2, |ctx| {
            ctx.modal_scope(
                "dialog",
                Rect::new(0, 3, 4, 1),
                ScopeOptions::default(),
                |_| {},
            );
            ctx.paint_widget(Paragraph::new("AFTE"), Rect::new(0, 2, 4, 1));
        });
    });

    assert_eq!(driver.row(0), "AFTE", "the viewport came back scrolled");
    assert_eq!(
        driver.row(2),
        "    ",
        "the paint landed in screen coordinates"
    );
}

/// The one-viewport rule counts the viewports on one layer. A modal left the
/// enclosing viewport behind, so it may open one of its own — a scroll area
/// inside a dialog inside a scroll area.
#[test]
fn a_viewport_inside_a_modal_inside_a_viewport_declares_and_paints() {
    let mut driver = Driver::<State, Msg>::new(6, 4);
    driver.render(&State, |ctx| {
        ctx.viewport(Rect::new(0, 0, 6, 2), 10, 4, |ctx| {
            ctx.modal_scope(
                "dialog",
                Rect::new(0, 0, 6, 4),
                ScopeOptions::default(),
                |ctx| {
                    ctx.viewport(Rect::new(0, 1, 6, 2), 4, 2, |ctx| {
                        ctx.paint_widget(
                            Paragraph::new("top\nhid\nAAA\nBBB"),
                            Rect::new(0, 1, 3, 4),
                        );
                    });
                },
            );
        });
    });

    assert_eq!(driver.row(1), "AAA   ");
    assert_eq!(driver.row(2), "BBB   ");
}

/// A popup escapes the clip and keeps the coordinates: it is projected out of
/// its viewport once, and what it declares is still in that viewport's
/// logical space. A viewport declared inside one is therefore a viewport
/// inside a viewport, and says so.
#[test]
fn a_viewport_inside_a_popup_inside_a_viewport_is_still_nested() {
    let mut driver = Driver::<State, Msg>::new(6, 4);
    let result = catch_unwind(AssertUnwindSafe(|| {
        driver.render(&State, |ctx| {
            ctx.viewport(Rect::new(0, 0, 6, 2), 10, 0, |ctx| {
                ctx.popup(
                    "panel",
                    Rect::new(0, 0, 6, 4),
                    PopupOptions::default(),
                    |ctx| {
                        ctx.viewport(Rect::new(0, 1, 6, 2), 4, 2, |_| {});
                    },
                );
            });
        });
    }));
    let panic = result.expect_err("a nested viewport must panic");
    assert!(
        panic_message(panic.as_ref()).contains("a viewport cannot be declared inside another"),
        "{}",
        panic_message(panic.as_ref())
    );
}
