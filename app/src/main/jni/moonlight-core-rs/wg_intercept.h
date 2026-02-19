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
 * Otherwise, falls through to the real libc sendto. */
#define sendto(s,b,l,f,a,al) wg_sendto(s,b,l,f,a,al)

/* ============================================================================
 * TCP interception
 * ============================================================================ */

/* Redirect send/recv to WG-aware implementations.
 * These check if the socket FD is WG-tracked (TCP through WireGuard);
 * if so, route through the virtual TCP stack. Otherwise, use real libc. */
#define send(s,b,l,f) wg_tcp_send(s,b,l,f)
#define recv(s,b,l,f) wg_tcp_recv(s,b,l,f)

#endif /* WG_INTERCEPT_H */
