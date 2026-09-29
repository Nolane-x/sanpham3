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


fun AndroidBleL2capServerDataPath.acceptPeerSession(
    timeoutMillis: Int,
    nodeId: Long,
    peerKey: ByteArray,
): AndroidPeerSession {
    val socket = accept(timeoutMillis)
    return AndroidPeerSession.server(
        input = socket.inputStream,
        output = socket.outputStream,
        transport = socket,
        nodeId = nodeId,
        peerKey = peerKey,
    )
}

fun AndroidBleL2capClientDataPath.connectPeerSession(
    peer: AndroidBlePeer,
    nodeId: Long,
    peerKey: ByteArray,
): AndroidPeerSession {
    val socket = connect(peer)
    return AndroidPeerSession.client(
        input = socket.inputStream,
        output = socket.outputStream,
        transport = socket,
        nodeId = nodeId,
        peerKey = peerKey,
    )
}


fun AndroidBluetoothRfcommServerDataPath.acceptPeerSession(
    timeoutMillis: Int,
    nodeId: Long,
    peerKey: ByteArray,
): AndroidPeerSession {
    val socket = accept(timeoutMillis)
    return AndroidPeerSession.server(
        input = socket.inputStream,
        output = socket.outputStream,
        transport = socket,
        nodeId = nodeId,
        peerKey = peerKey,
    )
}

fun AndroidBluetoothRfcommClientDataPath.connectPeerSession(
    peer: AndroidBluetoothClassicPeer,
    nodeId: Long,
    peerKey: ByteArray,
): AndroidPeerSession {
    val socket = connect(peer)
    return AndroidPeerSession.client(
        input = socket.inputStream,
        output = socket.outputStream,
        transport = socket,
        nodeId = nodeId,
        peerKey = peerKey,
    )
}
