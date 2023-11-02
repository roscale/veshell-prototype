use std::cell::RefCell;
use std::collections::HashMap;
use std::env::{remove_var, set_var};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use smithay::{delegate_compositor, delegate_dmabuf, delegate_output, delegate_seat, delegate_shm, delegate_xdg_shell};
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::input::ButtonState;
use smithay::backend::renderer::{ImportAll, Texture};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::input::pointer::{ButtonEvent, CursorImageStatus, MotionEvent, PointerHandle};
use smithay::reexports::calloop::{channel, Interest, LoopHandle, Mode, PostAction};
use smithay::reexports::calloop::channel::Event::Msg;
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::XdgToplevel;
use smithay::reexports::wayland_server::{Client, Display, DisplayHandle, Resource};
use smithay::reexports::wayland_server::protocol::{wl_buffer, wl_seat};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Buffer as BufferCoords, Clock, Monotonic, Rectangle, Serial, Size};
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{BufferAssignment, CompositorClientState, CompositorHandler, CompositorState, SurfaceAttributes, with_states};
use smithay::wayland::dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportError};
use smithay::wayland::shell::xdg;
use smithay::wayland::shell::xdg::{PopupSurface, PositionerState, SurfaceCachedState, ToplevelSurface, XdgShellHandler, XdgShellState};
use smithay::wayland::shm::{ShmHandler, ShmState};
use smithay::wayland::socket::ListeningSocketSource;
use tracing::{info, warn};

use crate::{Backend, CalloopData, ClientState};
use crate::flutter_engine::FlutterEngine;
use crate::flutter_engine::platform_channels::encodable_value::EncodableValue;
use crate::flutter_engine::platform_channels::method_call::MethodCall;
use crate::flutter_engine::platform_channels::method_channel::MethodChannel;
use crate::flutter_engine::platform_channels::method_result::MethodResult;
use crate::flutter_engine::platform_channels::standard_method_codec::StandardMethodCodec;
use crate::flutter_engine::wayland_messages::{SurfaceCommitMessage, XdgSurfaceCommitMessage};
use crate::mouse_button_tracker::FLUTTER_TO_LINUX_MOUSE_BUTTONS;
use crate::texture_swap_chain::TextureSwapChain;

pub struct ServerState<BackendData: Backend + 'static> {
    pub running: Arc<AtomicBool>,
    pub display_handle: DisplayHandle,
    pub loop_handle: LoopHandle<'static, CalloopData<BackendData>>,
    pub clock: Clock<Monotonic>,
    pub seat: Seat<ServerState<BackendData>>,
    pub seat_state: SeatState<ServerState<BackendData>>,
    pub pointer: PointerHandle<ServerState<BackendData>>,
    pub backend_data: Box<BackendData>,
    pub flutter_engine: Option<Box<FlutterEngine>>,
    pub next_view_id: u64,
    pub next_texture_id: i64,
    // space: Space<WindowElement>,

    pub mouse_position: (f64, f64),
    pub is_next_vblank_scheduled: bool,

    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub shm_state: ShmState,
    pub dmabuf_state: Option<DmabufState>,

    pub imported_dmabufs: Vec<Dmabuf>,
    pub gles_renderer: Option<GlesRenderer>,
    pub surfaces: HashMap<u64, WlSurface>,
    pub xdg_toplevels: HashMap<u64, XdgToplevel>,
    pub texture_ids_per_view_id: HashMap<u64, Vec<i64>>,
    pub view_id_per_texture_id: HashMap<i64, u64>,
    pub texture_swapchains: HashMap<i64, TextureSwapChain>,

    pub tx_platform_message: Option<channel::Sender<(MethodCall, Box<dyn MethodResult>)>>,
}

impl<BackendData: Backend + 'static> ServerState<BackendData> {
    pub fn get_new_view_id(&mut self) -> u64 {
        let view_id = self.next_view_id;
        self.next_view_id += 1;
        view_id
    }

    pub fn get_new_texture_id(&mut self) -> i64 {
        let texture_id = self.next_texture_id;
        self.next_texture_id += 1;
        texture_id
    }
}

impl<BackendData: Backend + 'static> ServerState<BackendData> {
    pub fn flutter_engine(&self) -> &FlutterEngine {
        self.flutter_engine.as_ref().unwrap()
    }
    pub fn flutter_engine_mut(&mut self) -> &mut FlutterEngine {
        self.flutter_engine.as_mut().unwrap()
    }
}

// Macros used to delegate protocol handling to types in the app state.
delegate_compositor!(@<BackendData: Backend + 'static> ServerState<BackendData>);
delegate_xdg_shell!(@<BackendData: Backend + 'static> ServerState<BackendData>);
delegate_shm!(@<BackendData: Backend + 'static> ServerState<BackendData>);
delegate_dmabuf!(@<BackendData: Backend + 'static> ServerState<BackendData>);
delegate_output!(@<BackendData: Backend + 'static> ServerState<BackendData>);
delegate_seat!(@<BackendData: Backend + 'static> ServerState<BackendData>);
// delegate_data_device!(App);

impl<BackendData: Backend + 'static> ServerState<BackendData> {
    pub fn new(
        display: Display<ServerState<BackendData>>,
        loop_handle: LoopHandle<'static, CalloopData<BackendData>>,
        backend_data: BackendData,
        dmabuf_state: Option<DmabufState>,
    ) -> ServerState<BackendData> {
        let display_handle = display.handle();
        let clock = Clock::new().expect("failed to initialize clock");
        let compositor_state = CompositorState::new::<Self>(&display_handle);
        let xdg_shell_state = XdgShellState::new::<Self>(&display_handle);
        let shm_state = ShmState::new::<Self>(&display_handle, vec![]);

        // init input
        let mut seat_state = SeatState::new();
        let seat_name = backend_data.seat_name();
        let mut seat = seat_state.new_wl_seat(&display_handle, seat_name.clone());
        seat.add_keyboard(Default::default(), 200, 200).unwrap();
        let pointer = seat.add_pointer();

        // init wayland clients
        let source = ListeningSocketSource::new_auto().unwrap();
        let socket_name = source.socket_name().to_string_lossy().into_owned();
        loop_handle
            .insert_source(source, |client_stream, _, data| {
                if let Err(err) = data
                    .state.display_handle
                    .insert_client(client_stream, Arc::new(ClientState::default()))
                {
                    warn!("Error adding wayland client: {}", err);
                };
            })
            .expect("Failed to init wayland socket source");

        info!(name = socket_name, "Listening on wayland socket");

        remove_var("DISPLAY");
        set_var("WAYLAND_DISPLAY", &socket_name);
        set_var("XDG_SESSION_TYPE", "wayland");
        set_var("GDK_BACKEND", "wayland"); // Force GTK apps to run on Wayland.
        set_var("QT_QPA_PLATFORM", "wayland"); // Force QT apps to run on Wayland.

        loop_handle
            .insert_source(
                Generic::new(display, Interest::READ, Mode::Level),
                |_, display, data| {
                    profiling::scope!("dispatch_clients");
                    // Safety: we don't drop the display
                    unsafe {
                        display.get_mut().dispatch_clients(&mut data.state).unwrap();
                    }
                    Ok(PostAction::Continue)
                },
            )
            .expect("Failed to init wayland server source");

        let (tx_platform_message, rx_platform_message) = channel::channel::<(MethodCall, Box<dyn MethodResult>)>();

        macro_rules! extract {
            ($e:expr, $p:path) => {
                match $e {
                    $p(value) => value,
                    _ => panic!("Failed to extract value"),
                }
            };
        }

        fn get_value<'a>(map: &'a EncodableValue, key: &str) -> &'a EncodableValue {
            let map = extract!(map, EncodableValue::Map);
            for (k, v) in map {
                if let EncodableValue::String(k) = k {
                    if k == key {
                        return v;
                    }
                }
            }
            panic!("Key {} not found in map", key);
        }

        loop_handle
            .insert_source(
                rx_platform_message,
                |event, (), data| {
                    if let Msg((method_call, mut result)) = event {
                        let pointer = data.state.pointer.clone();
                        let now = Duration::from(data.state.clock.now()).as_millis() as u32;

                        match method_call.method() {
                            "pointer_hover" => {
                                let args = method_call.arguments().unwrap();
                                let view_id = get_value(args, "view_id").long_value().unwrap();
                                let x = *extract!(get_value(args, "x"), EncodableValue::Double);
                                let y = *extract!(get_value(args, "y"), EncodableValue::Double);

                                if let Some(surface) = data.state.surfaces.get(&(view_id as u64)).cloned() {
                                    pointer.motion(
                                        &mut data.state,
                                        Some((surface.clone(), (0, 0).into())),
                                        &MotionEvent {
                                            location: (x, y).into(),
                                            serial: Serial::from(0), // TODO
                                            time: now,
                                        },
                                    );
                                    pointer.frame(&mut data.state);
                                    result.success(None);
                                } else {
                                    result.error(
                                        "surface_doesnt_exist".to_string(),
                                        format!("Surface {view_id} doesn't exist"),
                                        None,
                                    );
                                }
                            }
                            "pointer_exit" => {
                                pointer.motion(
                                    &mut data.state,
                                    None,
                                    &MotionEvent {
                                        location: (0.0, 0.0).into(),
                                        serial: Serial::from(0), // TODO
                                        time: now,
                                    },
                                );
                                result.success(None);
                            }
                            "mouse_button_event" => {
                                let args = method_call.arguments().unwrap();
                                let button = get_value(args, "button").long_value().unwrap();
                                let is_pressed = *extract!(get_value(args, "is_pressed"), EncodableValue::Bool);

                                pointer.button(
                                    &mut data.state,
                                    &ButtonEvent {
                                        serial: Serial::from(0), // TODO
                                        time: now,
                                        button: *FLUTTER_TO_LINUX_MOUSE_BUTTONS.get(&(button as u32)).unwrap() as u32,
                                        state: if is_pressed { ButtonState::Pressed } else { ButtonState::Released },
                                    },
                                );
                                pointer.frame(&mut data.state);
                                result.success(None);
                            }
                            "activate_window" => {
                                let args = method_call.arguments().unwrap();
                                let args = extract!(args, EncodableValue::List);
                                let view_id = args[0].long_value().unwrap();
                                let activate = extract!(args[1], EncodableValue::Bool);

                                if let Some(toplevel) = data.state.xdg_toplevels.get(&(view_id as u64)) {
                                    let toplevel = data.state.xdg_shell_state.get_toplevel(toplevel).unwrap();
                                    toplevel.with_pending_state(|state| {
                                        if activate {
                                            state.states.set(xdg_toplevel::State::Activated);
                                        } else {
                                            state.states.unset(xdg_toplevel::State::Activated);
                                        }
                                    });
                                    toplevel.send_configure();
                                    result.success(None);
                                } else {
                                    result.error(
                                        "surface_doesnt_exist".to_string(),
                                        format!("Surface {view_id} doesn't exist"),
                                        None,
                                    );
                                }
                            }
                            _ => {
                                result.success(None);
                            }
                        }
                    }
                },
            )
            .expect("Failed to init wayland server source");

        Self {
            running: Arc::new(AtomicBool::new(true)),
            display_handle,
            loop_handle,
            clock,
            backend_data: Box::new(backend_data),
            mouse_position: (0.0, 0.0),
            is_next_vblank_scheduled: false,
            compositor_state,
            xdg_shell_state,
            shm_state,
            flutter_engine: None,
            dmabuf_state,
            seat,
            seat_state,
            pointer,
            next_view_id: 1,
            next_texture_id: 1,
            imported_dmabufs: Vec::new(),
            gles_renderer: None,
            surfaces: HashMap::new(),
            xdg_toplevels: HashMap::new(),
            texture_ids_per_view_id: HashMap::new(),
            view_id_per_texture_id: HashMap::new(),
            texture_swapchains: HashMap::new(),
            tx_platform_message: Some(tx_platform_message),
        }
    }
}

impl<BackendData: Backend> BufferHandler for ServerState<BackendData> {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl<BackendData: Backend> XdgShellHandler for ServerState<BackendData> {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let view_id = with_states(surface.wl_surface(), |surface_data| {
            surface_data.data_map.get::<RefCell<MySurfaceState>>().unwrap().borrow().view_id
        });
        self.xdg_toplevels.insert(view_id, surface.xdg_toplevel().clone());

        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Activated);
        });
        surface.send_configure();
    }

    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {
        // Handle popup creation here
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {
        // Handle popup grab here
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        let view_id = with_states(surface.wl_surface(), |surface_data| {
            surface_data.data_map.get::<RefCell<MySurfaceState>>().unwrap().borrow().view_id
        });
        self.xdg_toplevels.remove(&view_id);
    }
}

pub struct MySurfaceState {
    pub view_id: u64,
    pub old_texture_size: Option<Size<i32, BufferCoords>>,
}

impl<BackendData: Backend> CompositorHandler for ServerState<BackendData> {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn new_surface(&mut self, surface: &WlSurface) {
        let view_id = self.get_new_view_id();
        with_states(surface, |surface_data| {
            surface_data.data_map.insert_if_missing(|| RefCell::new(MySurfaceState {
                view_id,
                old_texture_size: None,
            }));
        });
        self.surfaces.insert(view_id, surface.clone());
    }

    fn commit(&mut self, surface: &WlSurface) {
        // on_commit_buffer_handler::<Self>(surface);

        let commit_message = with_states(surface, |surface_data| {
            let role = surface_data.role;
            let state = surface_data.cached_state.current::<SurfaceAttributes>();
            let my_state = surface_data.data_map.get::<RefCell<MySurfaceState>>().unwrap();

            let (view_id, old_texture_size) = {
                let my_state = my_state.borrow();
                (my_state.view_id, my_state.old_texture_size)
            };

            let texture = state.buffer
                .as_ref()
                .and_then(|assignment| match assignment {
                    BufferAssignment::NewBuffer(buffer) => {
                        self.gles_renderer.as_mut().unwrap().import_buffer(buffer, Some(surface_data), &[]).and_then(|t| t.ok())
                    },
                    _ => None,
                });

            let (texture_id, size) = if let Some(texture) = texture {
                let size = texture.size();

                let size_changed = match old_texture_size {
                    Some(old_size) => old_size != size,
                    None => true,
                };

                my_state.borrow_mut().old_texture_size = Some(size);

                let texture_id = match size_changed {
                    true => None,
                    false => self.texture_ids_per_view_id.get(&view_id).and_then(|v| v.last()).cloned(),
                };

                let texture_id = texture_id.unwrap_or_else(|| {
                    let texture_id = self.get_new_texture_id();
                    while self.texture_ids_per_view_id.entry(view_id).or_default().len() >= 2 {
                        self.texture_ids_per_view_id.entry(view_id).or_default().remove(0);
                    }

                    self.texture_ids_per_view_id.entry(view_id).or_default().push(texture_id);
                    self.view_id_per_texture_id.insert(texture_id, view_id);
                    self.flutter_engine_mut().register_external_texture(texture_id).unwrap();
                    texture_id
                });

                let swapchain = self.texture_swapchains.entry(texture_id).or_default();
                swapchain.commit(texture.clone());

                self.flutter_engine_mut().mark_external_texture_frame_available(texture_id).unwrap();

                (texture_id, Some(size))
            } else {
                (-1, None)
            };

            SurfaceCommitMessage {
                view_id,
                role,
                texture_id: dbg!(texture_id),
                buffer_delta: state.buffer_delta,
                buffer_size: size,
                scale: state.buffer_scale,
                input_region: state.input_region.clone(),
                xdg_surface: match role {
                    Some(xdg::XDG_TOPLEVEL_ROLE | xdg::XDG_POPUP_ROLE) => {
                        let geometry = surface_data
                            .cached_state
                            .current::<SurfaceCachedState>()
                            .geometry;

                        Some(XdgSurfaceCommitMessage {
                            mapped: texture_id != -1,
                            role,
                            geometry: match geometry {
                                Some(geometry) => Some(geometry),
                                None => Some(Rectangle {
                                    loc: (0, 0).into(),
                                    size: match size {
                                        Some(size) => (size.w, size.h).into(),
                                        None => (0, 0).into(),
                                    },
                                }),
                            },
                        })
                    },
                    _ => None,
                },
            }
        });

        let commit_message = commit_message.serialize();

        let codec = Rc::new(StandardMethodCodec::new());
        let mut method_channel = MethodChannel::new(
            self.flutter_engine_mut().binary_messenger.as_mut().unwrap(),
            "platform".to_string(),
            codec,
        );
        method_channel.invoke_method("commit_surface", Some(Box::new(commit_message)), None);
    }

    fn destroyed(&mut self, _surface: &WlSurface) {
        let view_id = with_states(_surface, |surface_data| {
            surface_data.data_map.get::<RefCell<MySurfaceState>>().unwrap().borrow().view_id
        });
        self.surfaces.remove(&view_id);
    }
}

impl<BackendData: Backend> ShmHandler for ServerState<BackendData> {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl<BackendData: Backend> DmabufHandler for ServerState<BackendData> {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        self.dmabuf_state.as_mut().unwrap()
    }

    fn dmabuf_imported(&mut self, _global: &DmabufGlobal, _dmabuf: Dmabuf) -> Result<(), ImportError> {
        self.imported_dmabufs.push(_dmabuf);
        Ok(())
    }
}

// impl DmabufHandler for ServerState<X11Data> {
//     fn dmabuf_state(&mut self) -> &mut DmabufState {
//         &mut self.dmabuf_state.as_mut().unwrap()
//     }
//
//     fn dmabuf_imported(&mut self, _global: &DmabufGlobal, dmabuf: Dmabuf) -> Result<(), ImportError> {
//         self.backend_data
//             .gles_renderer
//             .import_dmabuf(&dmabuf, None)
//             .map(|_| ())
//             .map_err(|_| ImportError::Failed)
//     }
// }

impl<BackendData: Backend> SeatHandler for ServerState<BackendData> {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<ServerState<BackendData>> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, target: Option<&WlSurface>) {

    }
    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {

    }
}
