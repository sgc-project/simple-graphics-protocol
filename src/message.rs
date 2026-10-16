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
    /// One of the pipeline's overlay planes: hardware compositing on top of
    /// the primary, for a client that draws over somebody else's output.
    /// Not granted yet — a request for one is refused for the moment.
    Overlay,
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
}
