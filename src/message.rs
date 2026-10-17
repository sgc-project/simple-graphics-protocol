//! Messages and resource types used by the client-server protocol.
//!
//! Clients use [`ClientRequest`] to announce themselves, acquire resources,
//! release resources, and acknowledge server messages. Servers use
//! [`ServerMessage`] to advertise available resources, grant or deny requests,
//! and revoke previously granted resources.

use serde::{Deserialize, Serialize};

/// Which plane of a display pipeline a client asks for.
///
/// A pipeline is a CRTC, the connector it drives, and the planes that can
/// drive that CRTC. The role says which of those planes the client means to
/// draw on — the server grants resources per role.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum PlaneRole {
    /// The plane the display's owner scanouts through: the app on screen.
    #[default]
    Primary,
    /// One of the pipeline's overlay planes: hardware compositing, stacked
    /// above the primary, for a client that draws over somebody else's output
    /// or that wants a second plane of its own. A client that asks for one
    /// owns it until it releases the resource.
    Overlay,
}

/// The shape the pointer takes, as the server's cursor renderer draws it.
///
/// Named, not pixels: the shape set and its artwork belong to the graphics
/// controller, so every application and every toolkit looks the same. A client
/// says *which* shape, never what it looks like.
///
/// The discriminants are the wire format — new shapes are appended, never
/// renumbered, and a decoder that does not know one draws [`CursorShape::Default`].
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CursorShape {
    /// The ordinary pointer: what to use when nothing else applies.
    #[default]
    Default = 0,
    /// Over text the user can select or type into.
    Text = 1,
    /// Over something clickable: a button, a link.
    Pointer = 2,
    /// A precise position: drawing, aiming, cropping.
    Crosshair = 3,
    /// Something being dragged or repositioned.
    Move = 4,
    /// A vertical split or frame edge.
    ResizeVertical = 5,
    /// A horizontal split or frame edge.
    ResizeHorizontal = 6,
    /// The top-left / bottom-right diagonal.
    ResizeTopLeft = 7,
    /// The top-right / bottom-left diagonal.
    ResizeTopRight = 8,
    /// Working, nothing to click.
    Wait = 9,
    /// Working, but the application still responds.
    Progress = 10,
    /// Not allowed here: a forbidden drop, a disabled control.
    NotAllowed = 11,
    /// Context help is available.
    Help = 12,
    /// Something can be dragged from where the pointer is (an open hand).
    Grab = 13,
    /// Something is being dragged (the hand closed).
    Grabbing = 14,
}

/// What the server chose for a granted DRM plane role: the objects the client
/// is expected to program.
///
/// A primary grant hands over a pipeline — the client programs the mode on the
/// CRTC and draws through the primary plane. An overlay grant names one of that
/// pipeline's overlay planes, already stacked above the primary, which the
/// client drives with its own framebuffers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct DrmGrant {
    /// Which role was granted — the plane the client may draw on.
    pub plane: PlaneRole,
    /// The plane's kernel object id (`drmModeSetPlane`, or the `plane_id` of an
    /// atomic request).
    pub plane_id: u32,
    /// The CRTC that plane drives: the pipeline's, whose mode the primary
    /// client set. A plane commit needs this, never the connector.
    pub crtc_id: u32,
    /// The connector that CRTC drives, when it is known — informational, so a
    /// client can see which display it ended up on.
    #[serde(default)]
    pub connector_id: Option<u32>,
}

/// A resource that can be managed by the server, grouped by kind.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Resource {
    /// The framebuffer device (`/dev/fb0`).
    Fbdev,
    /// A DRM display card (`/dev/dri/cardN`) and the plane of it a client
    /// asks for. The granted fd is a lease: the holder can modeset on the
    /// objects it carries but never becomes DRM master, and the server can
    /// revoke it at any time.
    Drm {
        /// The card's index, as in `/dev/dri/cardN`.
        card: u8,
        /// Which plane of that card. Defaults to [`PlaneRole::Primary`], so a
        /// request written before roles existed still means "the display".
        #[serde(default)]
        plane: PlaneRole,
    },
    /// An input device.
    Input(InputResource),
}

impl Resource {
    /// A card's primary plane: the app on the display.
    pub fn drm(card: u8) -> Self {
        Self::Drm {
            card,
            plane: PlaneRole::Primary,
        }
    }

    /// One overlay plane of a card's pipeline: hardware compositing above
    /// whoever holds the primary plane.
    pub fn drm_overlay(card: u8) -> Self {
        Self::Drm {
            card,
            plane: PlaneRole::Overlay,
        }
    }
}

/// Input resources. The index counts devices of the same class, so the
/// registry can hold several mice/keyboards/touchscreens at once.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum InputResource {
    /// A mouse-like device (relative axes + buttons).
    Mouse(u8),
    /// A keyboard-like device (key codes).
    Keyboard(u8),
    /// A touch device (absolute axes, single- or multi-touch).
    Touch(u8),
}

/// A request sent from a client to the server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClientRequest {
    /// Requests ownership of one resource.
    Acquire {
        /// The resource the client wants to acquire.
        resource: Resource,
    },
    /// Releases one resource previously acquired by the client.
    Release {
        /// The resource the client wants to release.
        resource: Resource,
    },
    /// Acknowledges receipt of a [`ServerMessage::Grant`] and its file
    /// descriptors. The server waits for this within a timeout after
    /// granting; without it the server cannot tell a delivered grant from a
    /// lost one.
    Ack,
    /// Which shape the cursor takes, and whether it is shown at all.
    ///
    /// Sent on change only — a shape that did not change is not re-sent with
    /// every pointer motion, because the graphics controller owns the position
    /// and draws the cursor itself. `visible: false` is how an application
    /// that draws its own crosshair or brush hides the system cursor and puts
    /// its own mark in its content.
    ///
    /// Only the focused application's shape is honoured: a background window
    /// cannot change the pointer the user is looking at.
    CursorShape {
        /// The shape to draw.
        shape: CursorShape,
        /// Whether the cursor is shown.
        visible: bool,
    },
    /// Where this client's overlay is, or that it is hidden (all zeros). Sent
    /// when the geometry is known and whenever it changes: the daemon records
    /// it, so a stacking or focus policy can say which app is where without
    /// guessing from a plane state that only reflects the last commit.
    ///
    /// Only meaningful for an overlay grant; the app on the display owns the
    /// whole screen and has no geometry to report.
    OverlayGeometry {
        /// The overlay resource the geometry belongs to (its card is what makes
        /// the grant unique).
        resource: Resource,
        /// The reported rectangle, or the hidden marker.
        geometry: OverlayGeometry,
    },
}

/// Where a client's overlay sits on the screen, in CRTC pixels.
///
/// The daemon can read a plane's programmed rectangle from the kernel, but that
/// only says where the last commit put it — not whether the client considers
/// itself visible. `OverlayGeometry::default()` (all zeros) means hidden: the
/// plane is cleared and the client keeps its grant.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct OverlayGeometry {
    /// The plane's left edge in CRTC coordinates.
    pub x: i32,
    /// The plane's top edge in CRTC coordinates.
    pub y: i32,
    /// The plane's width in pixels (its buffer's width: a plane is 1:1).
    pub width: u32,
    /// The plane's height in pixels.
    pub height: u32,
}

impl OverlayGeometry {
    /// Whether the client is reporting itself hidden: a zero-sized window is
    /// not a window, so no visible geometry can map to this.
    pub fn is_hidden(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// A message sent from the server to a client.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ServerMessage {
    /// The resources the server currently offers, in priority order.
    ///
    /// Sent on connect, and again to every connected client whenever the list
    /// changes (a device plugged in, a device removed). It is the whole list,
    /// not a delta: a client replaces its view, so a missed push costs nothing.
    Advertise {
        /// Resources that are currently available for acquisition.
        available_resources: Vec<Resource>,
    },
    /// Grants the client's resource request, or hands it a resource it already
    /// holds with a fresh fd (the device behind it went away and came back).
    Grant {
        /// The resource granted for the client, with one fd.
        resource: Resource,
        /// For a DRM resource: which plane the server chose. A grant written
        /// before this field existed decodes as `None`, and a client that does
        /// not know it can still enumerate the planes in its lease.
        #[serde(default)]
        drm: Option<DrmGrant>,
    },
    /// Denies the client's request.
    Deny {
        /// Human-readable explanation for why the request was denied.
        reason: String,
    },
    /// Revokes a resource previously granted to the client.
    Revoke {
        /// The resource the client must release.
        resource: Resource,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serialization::{deserialize, serialize};

    /// The shape a client wrote before roles existed: the same variant, with
    /// no plane field. Named MessagePack maps plus `#[serde(default)]` are
    /// what keep it readable — the field is simply absent.
    #[derive(Serialize)]
    enum ResourceBeforeRoles {
        Drm { card: u8 },
    }

    #[test]
    fn a_cursor_shape_round_trips_and_keeps_its_wire_numbers() {
        let request = ClientRequest::CursorShape {
            shape: CursorShape::Text,
            visible: true,
        };
        let decoded: ClientRequest = deserialize(&serialize(&request).unwrap()).unwrap();
        assert_eq!(decoded, request);
        // The numbers are the wire format: a decoder from another build must
        // agree, so they are asserted, not implied by declaration order.
        assert_eq!(CursorShape::Default as u8, 0);
        assert_eq!(CursorShape::Text as u8, 1);
        assert_eq!(CursorShape::Pointer as u8, 2);
        assert_eq!(CursorShape::Help as u8, 12);
        assert_eq!(CursorShape::Grab as u8, 13);
        assert_eq!(CursorShape::Grabbing as u8, 14);
        assert_eq!(CursorShape::default(), CursorShape::Default);
    }

    #[test]
    fn an_overlay_geometry_round_trips_and_defaults_to_hidden() {
        let geometry = OverlayGeometry {
            x: 523,
            y: 264,
            width: 320,
            height: 240,
        };
        let decoded: ClientRequest = deserialize(
            &serialize(&ClientRequest::OverlayGeometry {
                resource: Resource::drm_overlay(0),
                geometry,
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            decoded,
            ClientRequest::OverlayGeometry {
                resource: Resource::drm_overlay(0),
                geometry,
            }
        );
        // The zero value is the hidden marker, not a valid window.
        assert!(OverlayGeometry::default().is_hidden());
        assert!(!geometry.is_hidden());
    }

    #[test]
    fn a_request_without_a_role_is_a_primary_request() {
        let bytes = serialize(&ResourceBeforeRoles::Drm { card: 3 }).unwrap();
        let decoded: Resource = deserialize(&bytes).unwrap();
        assert_eq!(decoded, Resource::drm(3));
        assert_eq!(
            decoded,
            Resource::Drm {
                card: 3,
                plane: PlaneRole::Primary
            }
        );
    }

    #[test]
    fn roles_survive_the_wire() {
        for resource in [Resource::drm(0), Resource::drm_overlay(0), Resource::Fbdev] {
            let bytes = serialize(&resource).unwrap();
            assert_eq!(deserialize::<Resource>(&bytes).unwrap(), resource);
        }
    }

    #[test]
    fn a_primary_and_an_overlay_request_are_different_resources() {
        // Ownership, queueing and revocation are keyed by resource, so the two
        // roles must not collide on a card.
        assert_ne!(Resource::drm(0), Resource::drm_overlay(0));
        assert_eq!(Resource::drm(0), Resource::drm(0));
    }

    /// The shape a server wrote before the plane detail existed: the same
    /// variant, with only the resource.
    #[derive(Serialize)]
    enum GrantBeforePlaneDetail {
        Grant { resource: Resource },
    }

    #[test]
    fn a_grant_without_plane_detail_is_readable() {
        let bytes = serialize(&GrantBeforePlaneDetail::Grant {
            resource: Resource::drm(0),
        })
        .unwrap();
        assert_eq!(
            deserialize::<ServerMessage>(&bytes).unwrap(),
            ServerMessage::Grant {
                resource: Resource::drm(0),
                drm: None,
            }
        );
    }

    #[test]
    fn an_overlay_grant_names_the_plane_to_draw_on() {
        let grant = DrmGrant {
            plane: PlaneRole::Overlay,
            plane_id: 44,
            crtc_id: 48,
            connector_id: Some(50),
        };
        let message = ServerMessage::Grant {
            resource: Resource::drm_overlay(0),
            drm: Some(grant),
        };
        let bytes = serialize(&message).unwrap();
        assert_eq!(deserialize::<ServerMessage>(&bytes).unwrap(), message);
        assert_ne!(grant.plane, PlaneRole::Primary);
    }

    #[test]
    fn a_grant_whose_connector_is_unknown_round_trips() {
        let grant = DrmGrant {
            plane: PlaneRole::Primary,
            plane_id: 40,
            crtc_id: 48,
            connector_id: None,
        };
        let bytes = serialize(&grant).unwrap();
        assert_eq!(deserialize::<DrmGrant>(&bytes).unwrap(), grant);
    }
}
