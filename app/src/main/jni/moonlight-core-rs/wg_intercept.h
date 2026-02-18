/*
 * WireGuard zero-copy socket interception header
 *
 * This header is force-included (-include) in moonlight-common-c compilation
 * to redirect sendto() calls to our WG-aware implementation.
 *
 * When the system header <sys/socket.h> is processed after this file,
 * the sendto declaration is automatically expanded to declare wg_sendto.
 *
 * Do NOT include this in PlatformSockets.c (it's compiled separately).
 */

#ifndef WG_INTERCEPT_H
#define WG_INTERCEPT_H

/* Redirect sendto to WG-aware implementation.
 * wg_sendto checks if the socket is WG-tracked and the destination
 * is the WG server; if so, encapsulates directly through WireGuard.
 * Otherwise, falls through to the real libc sendto. */
#define sendto(s,b,l,f,a,al) wg_sendto(s,b,l,f,a,al)

#endif /* WG_INTERCEPT_H */
