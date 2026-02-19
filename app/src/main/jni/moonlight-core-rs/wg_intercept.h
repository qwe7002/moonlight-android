/*
 * WireGuard zero-copy socket interception header
 *
 * This header is force-included (-include) in both moonlight-common-c and
 * PlatformSockets.c compilation to redirect socket calls to WG-aware implementations.
 *
 * When the system headers are processed after this file,
 * the declarations are automatically expanded to declare wg_* functions.
 */

#ifndef WG_INTERCEPT_H
#define WG_INTERCEPT_H

/* ============================================================================
 * UDP interception
 * ============================================================================ */

/* Redirect sendto to WG-aware implementation.
 * wg_sendto checks if the socket is WG-tracked and the destination
 * is the WG server; if so, encapsulates directly through WireGuard.
 * For unregistered sockets (e.g., ENet), auto-registers them for
 * inject-mode delivery. Otherwise, falls through to real libc sendto. */
#define sendto(s,b,l,f,a,al) wg_sendto(s,b,l,f,a,al)

/* Redirect recvfrom to WG-aware implementation.
 * For inject-mode sockets (e.g., ENet), fixes the source address from
 * localhost (injected) to the actual WG server address.
 * Otherwise, falls through to real libc recvfrom. */
#define recvfrom(s,b,l,f,a,al) wg_recvfrom(s,b,l,f,a,al)

/* Redirect connect to WG-aware implementation for UDP sockets.
 * For UDP sockets connecting to the WG server, we skip the real connect()
 * (which would filter incoming packets by source) and store the peer address.
 * This allows loopback-injected data to be received by the socket.
 * For non-UDP or non-WG destinations, passes through to real libc connect. */
#define connect(s,a,l) wg_udp_connect(s,a,l)

/* ============================================================================
 * TCP interception
 * ============================================================================ */

/* Redirect send/recv to WG-aware implementations.
 * These check if the socket FD is WG-tracked (TCP through WireGuard);
 * if so, route through the virtual TCP stack. Otherwise, use real libc. */
#define send(s,b,l,f) wg_tcp_send(s,b,l,f)
#define recv(s,b,l,f) wg_tcp_recv(s,b,l,f)

#endif /* WG_INTERCEPT_H */
