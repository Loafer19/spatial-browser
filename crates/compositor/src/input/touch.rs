// Touch → CEF send_touch_event, or canvas pan/pinch (Linux multi-touch path).

use cef::{BrowserHost, ImplBrowserHost, PointerType, TouchEvent, TouchEventType};
use std::collections::HashMap;
use winit::event::{Touch, TouchPhase};

#[derive(Clone, Copy)]
enum FingerTarget {
    /// Empty canvas — drives one-finger pan.
    Canvas,
    /// Page under the finger; `local` is DIP coords in that page's view.
    Page { local: (f32, f32), browser_id: i32 },
}

struct Finger {
    window: (f32, f32),
    target: FingerTarget,
}

struct TwoFingerGesture {
    start_mid: (f32, f32),
    start_dist: f32,
    start_offset: (f32, f32),
    start_zoom: f32,
}

#[derive(Default)]
pub struct TouchInput {
    fingers: HashMap<u64, Finger>,
    two_finger: Option<TwoFingerGesture>,
    /// One-finger empty-canvas pan: screen pos + viewport offset at start.
    canvas_pan: Option<((f32, f32), (f32, f32))>,
}

/// Hit result from the compositor for the touch's window position.
pub struct TouchHit {
    pub browser_id: i32,
    /// Logical (DIP) coords inside the page's CEF view.
    pub local: (f32, f32),
}

/// Commands the compositor applies after a touch sample.
pub enum TouchCmd {
    Send {
        browser_id: i32,
        id: u64,
        local: (f32, f32),
        type_: TouchEventType,
        pressure: f32,
    },
    PanViewport {
        offset: (f32, f32),
    },
    /// Absolute viewport write (two-finger pinch pan+zoom together).
    SetViewport {
        offset: (f32, f32),
        zoom: f32,
    },
}

impl TouchInput {
    pub fn handle(
        &mut self,
        touch: &Touch,
        hit: Option<TouchHit>,
        viewport_offset: (f32, f32),
        viewport_zoom: f32,
    ) -> Vec<TouchCmd> {
        let id = touch.id;
        let window = (touch.location.x as f32, touch.location.y as f32);
        let pressure = touch.force.map(|f| f.normalized() as f32).unwrap_or(1.0);
        let mut cmds = Vec::new();

        match touch.phase {
            TouchPhase::Started => {
                // Second+ finger: cancel page forwards (fingers stay tracked)
                // and steal the gesture for canvas pan/pinch.
                let stealing = !self.fingers.is_empty();
                if stealing {
                    for (fid, finger) in &mut self.fingers {
                        if let FingerTarget::Page { local, browser_id } = finger.target {
                            cmds.push(TouchCmd::Send {
                                browser_id,
                                id: *fid,
                                local,
                                type_: TouchEventType::CANCELLED,
                                pressure: 0.0,
                            });
                            finger.target = FingerTarget::Canvas;
                        }
                    }
                    self.two_finger = None;
                    self.canvas_pan = None;
                }

                let target = if stealing {
                    FingerTarget::Canvas
                } else if let Some(h) = hit {
                    FingerTarget::Page {
                        local: h.local,
                        browser_id: h.browser_id,
                    }
                } else {
                    FingerTarget::Canvas
                };

                if let FingerTarget::Page { local, browser_id } = target {
                    cmds.push(TouchCmd::Send {
                        browser_id,
                        id,
                        local,
                        type_: TouchEventType::PRESSED,
                        pressure,
                    });
                } else if !stealing {
                    self.canvas_pan = Some((window, viewport_offset));
                }

                self.fingers.insert(id, Finger { window, target });

                if self.fingers.len() >= 2 {
                    self.canvas_pan = None;
                    self.begin_two_finger(viewport_offset, viewport_zoom);
                }
            }
            TouchPhase::Moved => {
                if let Some(finger) = self.fingers.get_mut(&id) {
                    finger.window = window;
                    if let FingerTarget::Page {
                        local,
                        browser_id,
                    } = &mut finger.target
                    {
                        if let Some(h) = hit {
                            *local = h.local;
                            *browser_id = h.browser_id;
                            cmds.push(TouchCmd::Send {
                                browser_id: h.browser_id,
                                id,
                                local: h.local,
                                type_: TouchEventType::MOVED,
                                pressure,
                            });
                        }
                    }
                }

                if let Some((start_pos, start_offset)) = self.canvas_pan {
                    if self.fingers.len() == 1 {
                        let dx = window.0 - start_pos.0;
                        let dy = window.1 - start_pos.1;
                        cmds.push(TouchCmd::PanViewport {
                            offset: (
                                start_offset.0 - dx / viewport_zoom,
                                start_offset.1 - dy / viewport_zoom,
                            ),
                        });
                    }
                }

                if self.fingers.len() >= 2 {
                    if self.two_finger.is_none() {
                        self.begin_two_finger(viewport_offset, viewport_zoom);
                    }
                    if let Some(cmd) = self.two_finger_cmd() {
                        cmds.push(cmd);
                    }
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                let ty = if touch.phase == TouchPhase::Cancelled {
                    TouchEventType::CANCELLED
                } else {
                    TouchEventType::RELEASED
                };
                if let Some(finger) = self.fingers.remove(&id) {
                    if let FingerTarget::Page { local, browser_id } = finger.target {
                        cmds.push(TouchCmd::Send {
                            browser_id,
                            id,
                            local,
                            type_: ty,
                            pressure,
                        });
                    }
                }
                if self.fingers.len() < 2 {
                    self.two_finger = None;
                }
                if self.fingers.is_empty() {
                    self.two_finger = None;
                    self.canvas_pan = None;
                } else if self.fingers.len() == 1 {
                    // Left over from a pinch — don't inherit a canvas pan.
                    self.canvas_pan = None;
                    if let Some(f) = self.fingers.values_mut().next() {
                        f.target = FingerTarget::Canvas;
                    }
                }
            }
        }
        cmds
    }

    fn begin_two_finger(&mut self, viewport_offset: (f32, f32), viewport_zoom: f32) {
        let pts: Vec<_> = self.fingers.values().map(|f| f.window).collect();
        if pts.len() < 2 {
            return;
        }
        let mid = ((pts[0].0 + pts[1].0) * 0.5, (pts[0].1 + pts[1].1) * 0.5);
        let dist = distance(pts[0], pts[1]).max(1.0);
        self.two_finger = Some(TwoFingerGesture {
            start_mid: mid,
            start_dist: dist,
            start_offset: viewport_offset,
            start_zoom: viewport_zoom,
        });
    }

    fn two_finger_cmd(&self) -> Option<TouchCmd> {
        let g = self.two_finger.as_ref()?;
        let pts: Vec<_> = self.fingers.values().map(|f| f.window).collect();
        if pts.len() < 2 {
            return None;
        }
        let mid = ((pts[0].0 + pts[1].0) * 0.5, (pts[0].1 + pts[1].1) * 0.5);
        let dist = distance(pts[0], pts[1]).max(1.0);
        let zoom = (g.start_zoom * (dist / g.start_dist)).clamp(0.2, 3.0);
        let world_pivot = (
            g.start_mid.0 / g.start_zoom + g.start_offset.0,
            g.start_mid.1 / g.start_zoom + g.start_offset.1,
        );
        let offset = (world_pivot.0 - mid.0 / zoom, world_pivot.1 - mid.1 / zoom);
        Some(TouchCmd::SetViewport { offset, zoom })
    }
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    (dx * dx + dy * dy).sqrt()
}

/// Forward a touch sample into CEF for one page.
pub fn send_to_host(
    host: &BrowserHost,
    id: u64,
    local: (f32, f32),
    type_: TouchEventType,
    pressure: f32,
) {
    let event = TouchEvent {
        id: id as i32,
        x: local.0,
        y: local.1,
        radius_x: 0.0,
        radius_y: 0.0,
        rotation_angle: 0.0,
        pressure,
        type_,
        modifiers: 0,
        pointer_type: PointerType::TOUCH,
    };
    host.send_touch_event(Some(&event));
}
