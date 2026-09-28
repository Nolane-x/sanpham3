package dev.nolane.sanpham3.androidhost

fun AndroidWifiDirectDataPath.acceptPeerSession(
    timeoutMillis: Int,
    nodeId: Long,
    peerKey: ByteArray,
): AndroidPeerSession =
    AndroidPeerSession.server(
        socket = accept(timeoutMillis),
        nodeId = nodeId,
        peerKey = peerKey,
    )

fun AndroidWifiDirectDataPath.connectPeerSession(
    timeoutMillis: Int,
    nodeId: Long,
    peerKey: ByteArray,
): AndroidPeerSession =
    AndroidPeerSession.client(
        socket = connect(timeoutMillis),
        nodeId = nodeId,
        peerKey = peerKey,
    )

fun AndroidWifiAwareServerDataPath.acceptPeerSession(
    timeoutMillis: Int,
    nodeId: Long,
    peerKey: ByteArray,
): AndroidPeerSession =
    AndroidPeerSession.server(
        socket = accept(timeoutMillis),
        nodeId = nodeId,
        peerKey = peerKey,
    )

fun AndroidWifiAwareClientDataPath.connectPeerSession(
    timeoutMillis: Int,
    nodeId: Long,
    peerKey: ByteArray,
): AndroidPeerSession =
    AndroidPeerSession.client(
        socket = connect(timeoutMillis),
        nodeId = nodeId,
        peerKey = peerKey,
    )
