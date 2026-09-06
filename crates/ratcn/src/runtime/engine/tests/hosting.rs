//! A hosted tree bounds floating layers without sandboxing base paint.

use ratatui::{
    style::Color,
    text::Line,
    widgets::{Block, Paragraph},
};

use super::*;
use crate::{ListItem, Select, Tooltip, TooltipSide};

const ROOT: Rect = Rect::new(8, 4, 18, 7);

#[test]
fn overlapping_style_paint_preserves_a_complete_wide_glyph() {
    for root in [Rect::new(0, 0, 40, 18), ROOT] {
        let mut driver = Driver::<(), ()>::new(40, 18);
        driver
            .terminal
            .draw(|frame| {
                driver
                    .ratcn
                    .render(frame, root, &(), &Theme::default_dark(), |ctx| {
                        ctx.modal_scope(
                            "modal",
                            Rect::new(2, 1, 34, 14),
                            ScopeOptions::default(),
                            |ctx| {
                                ctx.paint_widget(Line::from("\u{754c}"), Rect::new(10, 7, 2, 1));
                                ctx.paint_widget(
                                    Block::new().style(Color::Green),
                                    Rect::new(10, 7, 1, 1),
                                );
                            },
                        );
                    });
                let blank = Buffer::empty(frame.area());
                let updates = blank.diff(frame.buffer_mut());
                let &(_, _, cell) = updates
                    .iter()
                    .find(|&&(x, y, _)| x == 10 && y == 7)
                    .expect("the restyled glyph must reach the terminal");
                assert_eq!(
                    cell.symbol(),
                    "\u{754c}",
                    "paint rectangles are not clipping boundaries: {root:?}"
                );
                assert_eq!(cell.cell_width(), 2);
                assert_eq!(cell.fg, Color::Green);
                assert!(!updates.iter().any(|&(x, y, _)| x == 11 && y == 7));
            })
            .expect("draw");
    }
}

#[test]
fn clipped_modal_wide_glyph_does_not_emit_over_host_chrome() {
    // A split glyph on either edge becomes blank; whole glyphs at the edges survive.
    for (glyph_x, visible) in [(25, false), (24, true), (7, false), (8, true)] {
        let mut driver = Driver::<(), ()>::new(40, 18);
        driver
            .terminal
            .draw(|frame| {
                for cell in &mut frame.buffer_mut().content {
                    cell.set_symbol("#");
                }
                let before = frame.buffer_mut().clone();
                driver
                    .ratcn
                    .render(frame, ROOT, &(), &Theme::default_dark(), |ctx| {
                        ctx.modal_scope(
                            "modal",
                            Rect::new(2, 1, 34, 14),
                            ScopeOptions::default(),
                            |ctx| {
                                ctx.paint_widget(
                                    Line::from("\u{754c}").style(Color::Green),
                                    Rect::new(glyph_x, 7, 2, 1),
                                );
                            },
                        );
                    });
                for position in before
                    .area
                    .positions()
                    .filter(|point| !ROOT.contains(*point))
                {
                    assert_eq!(frame.buffer_mut()[position], before[position]);
                }
                let blank = Buffer::empty(before.area);
                // Check both the initial terminal draw and a redraw over existing chrome.
                for previous in [&blank, &before] {
                    let updates = previous.diff(frame.buffer_mut());
                    for &(x, y, cell) in &updates {
                        if ROOT.contains(Position::new(x, y)) {
                            assert!(
                                x + cell.cell_width() <= ROOT.right(),
                                "diff emits {:?} at ({x}, {y}), width {}; host x=26 emitted: {}",
                                cell.symbol(),
                                cell.cell_width(),
                                updates.iter().any(|&(x, y, _)| x == 26 && y == 7)
                            );
                        }
                    }
                    assert_eq!(
                        updates.iter().any(|&(x, y, cell)| x == glyph_x
                            && y == 7
                            && cell.symbol() == "\u{754c}"),
                        visible,
                        "only whole glyphs should reach the terminal, source x={glyph_x}"
                    );
                }
                let edge = glyph_x.max(ROOT.x);
                assert_eq!(
                    frame.buffer_mut()[(edge, 7)].symbol(),
                    if visible { "\u{754c}" } else { " " }
                );
                if glyph_x >= ROOT.x {
                    assert_eq!(frame.buffer_mut()[(edge, 7)].fg, Color::Green);
                }
            })
            .expect("draw");
        assert_eq!(driver.cell(7, 7).symbol(), "#");
        assert_eq!(driver.cell(26, 7).symbol(), "#");
    }
}

fn render_hosted(
    declare: impl FnOnce(&mut DeclareCtx<'_, (), &'static str>),
) -> Driver<(), &'static str> {
    let mut driver = Driver::new(40, 18);
    driver
        .terminal
        .draw(|frame| {
            for cell in &mut frame.buffer_mut().content {
                cell.set_symbol("#")
                    .set_fg(Color::Yellow)
                    .set_bg(Color::Blue);
            }
            let before = frame.buffer_mut().clone();
            driver
                .ratcn
                .render(frame, ROOT, &(), &Theme::default_dark(), |ctx| {
                    assert_eq!(ctx.area(), ROOT);
                    assert_eq!(ctx.frame_area(), ROOT);
                    declare(ctx);
                });
            for position in before
                .area
                .positions()
                .filter(|point| !ROOT.contains(*point))
            {
                assert_eq!(
                    frame.buffer_mut()[position],
                    before[position],
                    "host chrome changed at {position:?}"
                );
            }
        })
        .expect("draw");
    driver
}

#[test]
fn select_at_the_bottom_of_an_offset_root_keeps_its_options_visible_inside() {
    let mut driver = render_hosted(|ctx| {
        ctx.component(
            "fruit",
            Select::new(["Mango", "Papaya", "Lychee"].map(|label| ListItem::new(label, label)))
                .open(|(): &()| true, |_| "open")
                .selection(|(): &()| None, |value| value),
            Rect::new(ROOT.x + 1, ROOT.bottom() - 1, 14, 1),
        );
    });

    // Clipping a panel still placed against the terminal would lose options.
    for (row, label) in [(7, "Mango"), (8, "Papaya"), (9, "Lychee")] {
        assert!(driver.row(row).contains(label), "{}", driver.row(row));
    }
    assert_eq!(
        driver.event(
            mouse(MouseKind::Click(MouseButton::Left), ROOT.x + 4, 8),
            &()
        ),
        EventResult::Emit("Papaya"),
        "the relocated option still accepts absolute screen coordinates"
    );
}

#[test]
fn tooltip_at_the_top_of_an_offset_root_flips_below_and_clamps_horizontally() {
    let mut driver = render_hosted(|ctx| {
        ctx.component(
            "tip",
            Tooltip::new("Save the file")
                .side(TooltipSide::Top)
                .open_when(|(): &(), _| true)
                .trigger(|ctx| {
                    ctx.component("save", Button::new("Save").on_press(|| "save"), ctx.area());
                }),
            Rect::new(ROOT.x, ROOT.y, 6, 1),
        );
    });

    assert!(driver.row(ROOT.y).contains("Save"));
    assert!(
        driver.row(ROOT.y + 2).contains("Save the file"),
        "the whole explanation must move below the trigger, not merely be clipped"
    );
    assert_eq!(
        driver.event(
            mouse(MouseKind::Click(MouseButton::Left), ROOT.x + 1, ROOT.y),
            &()
        ),
        EventResult::Emit("save")
    );
}

#[test]
fn oversized_modal_dims_and_copies_only_inside_the_root() {
    let driver = render_hosted(|ctx| {
        ctx.modal_scope(
            "modal",
            Rect::new(2, 1, 34, 14),
            ScopeOptions::default(),
            |ctx| {
                assert_eq!(ctx.frame_area(), ROOT);
                ctx.paint_widget(Line::from("M".repeat(34)), Rect::new(2, 7, 34, 1));
                ctx.paint_widget(Paragraph::new("V\n".repeat(14)), Rect::new(9, 1, 1, 14));
            },
        );
    });

    for position in ROOT.positions() {
        let cell = &driver.buffer()[position];
        if position.x == 9 {
            assert_eq!(
                cell.symbol(),
                "V",
                "the vertical strip must composite inside"
            );
        } else if position.y == 7 {
            assert_eq!(cell.symbol(), "M", "the modal must still composite inside");
        } else {
            assert_eq!(cell.symbol(), "#", "unpainted backdrop retains its glyphs");
            assert_ne!(cell.fg, Color::Yellow, "backdrop foreground must dim");
            assert_ne!(cell.bg, Color::Blue, "backdrop background must dim");
        }
    }
}

#[test]
fn viewport_popup_keeps_logical_root_bounds_and_projects_once() {
    let mut driver = render_hosted(|ctx| {
        ctx.viewport(Rect::new(10, 6, 12, 3), 12, 3, |ctx| {
            let logical_root = Rect::new(8, 7, 18, 7);
            assert_eq!(ctx.area(), Rect::new(10, 6, 12, 12));
            assert_eq!(ctx.frame_area(), logical_root);
            ctx.scope(
                "anchor",
                Rect::new(10, 9, 8, 1),
                ScopeOptions::default(),
                |ctx| {
                    assert_eq!(ctx.frame_area(), logical_root);
                    ctx.popup("popup", logical_root, PopupOptions::default(), |ctx| {
                        assert_eq!(ctx.area(), logical_root);
                        assert_eq!(ctx.frame_area(), logical_root);
                        ctx.component(
                            "button",
                            Button::new("Go").on_press(|| "popup"),
                            Rect::new(10, 9, 6, 1),
                        );
                        ctx.defer_paint(move |ctx| {
                            assert_eq!(ctx.area(), logical_root);
                            ctx.widget(Line::from("T"), Rect::new(8, 7, 1, 1));
                            ctx.widget(Line::from("B"), Rect::new(8, 13, 1, 1));
                        });
                    });
                },
            );
        });
    });

    assert_eq!(driver.cell(ROOT.x, ROOT.y).symbol(), "T");
    assert_eq!(driver.cell(ROOT.x, ROOT.bottom() - 1).symbol(), "B");
    assert!(driver.row(6).contains("Go"));
    assert_eq!(
        driver.event(mouse(MouseKind::Click(MouseButton::Left), 11, 6), &()),
        EventResult::Emit("popup"),
        "popup input is translated once, just like its paint"
    );
}

#[test]
fn viewport_modal_restores_screen_root_bounds_and_can_open_its_own_viewport() {
    let mut driver = render_hosted(|ctx| {
        ctx.viewport(Rect::new(10, 6, 12, 3), 12, 3, |ctx| {
            ctx.modal_scope(
                "modal",
                Rect::new(11, 10, 8, 3),
                ScopeOptions::default(),
                |ctx| {
                    assert_eq!(ctx.area(), Rect::new(11, 7, 8, 3));
                    assert_eq!(ctx.frame_area(), ROOT);
                    ctx.viewport(Rect::new(11, 7, 8, 2), 6, 2, |ctx| {
                        assert_eq!(ctx.frame_area(), Rect::new(8, 6, 18, 7));
                        ctx.component(
                            "button",
                            Button::new("Go").on_press(|| "modal"),
                            Rect::new(11, 9, 6, 1),
                        );
                    });
                },
            );
            assert_eq!(ctx.frame_area(), Rect::new(8, 7, 18, 7));
        });
        assert_eq!(ctx.frame_area(), ROOT);
    });

    assert!(driver.row(7).contains("Go"));
    assert_eq!(
        driver.event(mouse(MouseKind::Click(MouseButton::Left), 12, 7), &()),
        EventResult::Emit("modal")
    );
}

#[test]
fn root_area_guides_deferred_paint_but_does_not_sandbox_base_writes() {
    let mut driver = Driver::<(), ()>::new(40, 18);
    driver
        .terminal
        .draw(|frame| {
            let destination = frame.area();
            driver
                .ratcn
                .render(frame, ROOT, &(), &Theme::default_dark(), |ctx| {
                    ctx.paint_widget(Line::from("W"), Rect::new(1, 0, 1, 1));
                    ctx.defer_paint(move |ctx| {
                        assert_eq!(ctx.area(), ROOT);
                        ctx.with_buffer(|buffer| {
                            assert_eq!(buffer.area, destination);
                            buffer[(0, 0)].set_symbol("D");
                        });
                    });
                });
        })
        .expect("draw");

    assert_eq!(driver.cell(0, 0).symbol(), "D");
    assert_eq!(driver.cell(1, 0).symbol(), "W");
}
