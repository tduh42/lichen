package com.tduhamel.lichen.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.material.icons.outlined.DeviceUnknown
import androidx.compose.material.icons.outlined.Forum
import androidx.compose.material.icons.outlined.MoreVert
import androidx.compose.material.icons.outlined.Person
import androidx.compose.material.icons.outlined.SettingsInputAntenna
import androidx.compose.material.icons.outlined.Thermostat
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.R
import com.tduhamel.lichen.core.ChannelCode
import com.tduhamel.lichen.core.ChannelKind
import com.tduhamel.lichen.core.LinkKind
import com.tduhamel.lichen.core.MeshChannel
import com.tduhamel.lichen.core.NodeInfo
import com.tduhamel.lichen.core.NodeKind
import com.tduhamel.lichen.core.PathHop
import com.tduhamel.lichen.core.SavedRadio
import com.tduhamel.lichen.core.ServerLogin
import com.tduhamel.lichen.core.ServerStats
import com.tduhamel.lichen.core.TraceInfo
import com.tduhamel.lichen.core.TraceState
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.canSwitchTo
import com.tduhamel.lichen.model.channelQr
import com.tduhamel.lichen.model.display
import com.tduhamel.lichen.model.forgetRadio
import com.tduhamel.lichen.model.heard
import com.tduhamel.lichen.model.hex
import com.tduhamel.lichen.model.node
import com.tduhamel.lichen.model.nodePath
import com.tduhamel.lichen.model.parseChannelQr
import com.tduhamel.lichen.model.refreshSavedRadios
import com.tduhamel.lichen.model.requestNeighbours
import com.tduhamel.lichen.model.requestServerStats
import com.tduhamel.lichen.model.sendServerCommand
import com.tduhamel.lichen.model.server
import com.tduhamel.lichen.model.serverLogin
import com.tduhamel.lichen.model.serverLogout
import com.tduhamel.lichen.model.startTrace
import com.tduhamel.lichen.model.switchRadio
import com.tduhamel.lichen.model.trace
import com.tduhamel.lichen.model.unhex
import java.text.DateFormat
import java.util.Date

// Mesh tools (protocol.md §15.8, §16; ADR-0041 to ADR-0044): the nodes the radio knows, a
// node's path and trace, room servers and repeaters, saved radios, channels by QR code.

fun nodeKindLabel(res: android.content.res.Resources, kind: NodeKind): String = res.getString(
    when (kind) {
        NodeKind.UNKNOWN -> R.string.node_kind_unknown
        NodeKind.COMPANION -> R.string.node_kind_companion
        NodeKind.REPEATER -> R.string.node_kind_repeater
        NodeKind.ROOM_SERVER -> R.string.node_kind_room
        NodeKind.SENSOR -> R.string.node_kind_sensor
    },
)

fun nodeKindIcon(kind: NodeKind): ImageVector = when (kind) {
    NodeKind.UNKNOWN -> Icons.Outlined.DeviceUnknown
    NodeKind.COMPANION -> Icons.Outlined.Person
    NodeKind.REPEATER -> Icons.Outlined.SettingsInputAntenna
    NodeKind.ROOM_SERVER -> Icons.Outlined.Forum
    NodeKind.SENSOR -> Icons.Outlined.Thermostat
}

fun loginLabel(res: android.content.res.Resources, login: ServerLogin): String = res.getString(
    when (login) {
        ServerLogin.OUT -> R.string.login_out
        ServerLogin.PENDING -> R.string.login_pending
        ServerLogin.GUEST -> R.string.login_guest
        ServerLogin.READ_ONLY -> R.string.login_read_only
        ServerLogin.READ_WRITE -> R.string.login_read_write
        ServerLogin.ADMIN -> R.string.login_admin
        ServerLogin.REFUSED -> R.string.login_refused
        ServerLogin.NO_ANSWER -> R.string.login_no_answer
    },
)

private val ServerLogin.isIn: Boolean
    get() = this == ServerLogin.GUEST || this == ServerLogin.READ_ONLY || this == ServerLogin.READ_WRITE || this == ServerLogin.ADMIN

private fun hopsText(res: android.content.res.Resources, hops: UByte?): String = when (hops) {
    null -> res.getString(R.string.hops_flood)
    0.toUByte() -> res.getString(R.string.hops_direct)
    else -> res.getQuantityString(R.plurals.hops_n, hops.toInt(), hops.toInt())
}

/** A room chat's subtitle: what it is, and the login (§15.8). */
@Composable
fun roomSubtitle(model: AppModel, chat: ChatId): String {
    val login = model.server(chat.hex.unhex())?.login ?: ServerLogin.OUT
    return stringResource(R.string.meshcore_room_login, loginLabel(LocalResources.current, login))
}

/** A screen of the mesh tools: a title bar with a back arrow over a list. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ToolScreen(title: String, back: () -> Unit, content: LazyListScope.() -> Unit) {
    val t = lichen
    Column(Modifier.fillMaxSize().background(t.bg)) {
        TopAppBar(
            navigationIcon = { IconButton(onClick = back) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) } },
            title = { Text(title, fontSize = 22.sp) },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = t.bg, titleContentColor = t.tx, navigationIconContentColor = t.tx),
        )
        LazyColumn(
            Modifier.weight(1f).windowInsetsPadding(WindowInsets.navigationBars),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
            content = content,
        )
    }
}

@Composable
private fun Note(text: String) {
    Text(text, fontSize = 12.sp, color = lichen.tx2, modifier = Modifier.padding(horizontal = 4.dp))
}

@Composable
private fun Chevron() = Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, tint = lichen.tx2)

// ----- nodes (§16.1) -----

private enum class NodeFilter(val label: Int) {
    ALL(R.string.filter_all), REPEATERS(R.string.filter_repeaters), ROOMS(R.string.filter_rooms), PEOPLE(R.string.filter_people);

    fun keeps(n: NodeInfo): Boolean = when (this) {
        ALL -> true
        REPEATERS -> n.kind == NodeKind.REPEATER
        ROOMS -> n.kind == NodeKind.ROOM_SERVER
        PEOPLE -> n.kind == NodeKind.COMPANION || n.kind == NodeKind.UNKNOWN
    }
}

/** Settings › Nodes: what the radio knows or heard, the most recently heard first. */
@Composable
fun NodesScreen(model: AppModel, back: () -> Unit, open: (ByteArray) -> Unit) {
    val t = lichen
    var filter by remember { mutableStateOf(NodeFilter.ALL) }
    var query by remember { mutableStateOf("") }
    val shown = model.nodes.filter { filter.keeps(it) && (query.isBlank() || it.name.contains(query.trim(), ignoreCase = true)) }
    ToolScreen(stringResource(R.string.nodes), back) {
        item {
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                NodeFilter.entries.forEach { f ->
                    FilterChip(
                        selected = filter == f,
                        onClick = { filter = f },
                        label = { Text(stringResource(f.label)) },
                        colors = FilterChipDefaults.filterChipColors(
                            containerColor = t.s1, labelColor = t.tx,
                            selectedContainerColor = t.actBg, selectedLabelColor = t.actFg,
                        ),
                    )
                }
            }
        }
        item {
            OutlinedTextField(
                query, { query = it }, Modifier.fillMaxWidth(), singleLine = true,
                placeholder = { Text(stringResource(R.string.name)) },
                shape = RoundedCornerShape(12.dp), colors = fieldColors(),
            )
        }
        if (model.nodes.isEmpty()) {
            item {
                Text(
                    stringResource(if (model.isConnected) R.string.nodes_none_yet else R.string.nodes_need_radio),
                    color = t.tx2, modifier = Modifier.padding(top = 20.dp),
                )
            }
        } else {
            item {
                CardList {
                    shown.forEachIndexed { i, n ->
                        if (i > 0) CardDivider()
                        NodeRow(model, n) { open(n.key) }
                    }
                }
            }
            item { Note(stringResource(R.string.nodes_note)) }
        }
    }
}

@Composable
private fun NodeRow(model: AppModel, n: NodeInfo, open: () -> Unit) {
    val t = lichen
    val res = LocalResources.current
    Row(
        Modifier.fillMaxWidth().clickable(onClick = open).padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Box(Modifier.size(36.dp).background(t.s2, RoundedCornerShape(9.dp)), contentAlignment = Alignment.Center) {
            Icon(nodeKindIcon(n.kind), null, tint = if (n.kind == NodeKind.REPEATER) t.lora else t.tx2)
        }
        Column(Modifier.weight(1f)) {
            Text(n.name, fontSize = 16.sp, color = t.tx)
            Text(
                listOf(nodeKindLabel(res, n.kind), hopsText(res, n.hops), model.heard(n.lastHeardS)).joinToString(" · "),
                fontSize = 13.sp, color = t.tx2,
            )
        }
        if (!n.onRadio) Badge(stringResource(R.string.heard_badge), t.s2, t.tx2)
        Chevron()
    }
}

/** One node: its path and trace; a room's login; a repeater's administration (§16). */
@Composable
fun NodeScreen(model: AppModel, key: ByteArray, back: () -> Unit, console: () -> Unit, openChat: (ChatId) -> Unit) {
    val t = lichen
    val res = LocalResources.current
    val n = model.node(key)
    var error by remember { mutableStateOf<String?>(null) }
    ToolScreen(n?.name ?: stringResource(R.string.node_kind_unknown), back) {
        if (n != null) item {
            CardList {
                CardRow(
                    nodeKindLabel(res, n.kind),
                    listOfNotNull(
                        if (n.onRadio) null else stringResource(R.string.heard_not_added),
                        stringResource(R.string.last_heard, model.heard(n.lastHeardS)),
                    ).joinToString(" · "),
                )
                if (n.lat != null && n.lon != null) {
                    CardDivider()
                    CardRow(stringResource(R.string.advertised_position), "%.5f, %.5f".format(n.lat, n.lon))
                }
                CardDivider()
                CardRow(stringResource(R.string.key), key.copyOf(8).hex())
            }
        }
        item { SectionLabel(stringResource(R.string.path), Modifier.padding(top = 10.dp)) }
        item { PathCard(model, key) }
        item { TraceCard(model, key) { error = it } }
        if (n?.kind == NodeKind.ROOM_SERVER) {
            item { SectionLabel(stringResource(R.string.room), Modifier.padding(top = 10.dp)) }
            item {
                CardList {
                    Box(Modifier.clickable { openChat(ChatId.room(key)) }) {
                        CardRow(stringResource(R.string.open_room), stringResource(R.string.open_room_note)) { Chevron() }
                    }
                }
            }
            item { ServerLoginCard(model, key, room = true) }
        }
        if (n?.kind == NodeKind.REPEATER) {
            item { SectionLabel(stringResource(R.string.administration), Modifier.padding(top = 10.dp)) }
            item { ServerLoginCard(model, key, room = false) }
            repeaterItems(model, key, console) { error = it }
        }
    }
    error?.let {
        AlertDialog(
            onDismissRequest = { error = null },
            title = { Text(stringResource(R.string.not_sent)) },
            text = { Text(it) },
            confirmButton = { TextButton(onClick = { error = null }) { Text(stringResource(R.string.ok), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}

/** The radio's path to a node, hop by hop (§16.2). */
@Composable
private fun PathCard(model: AppModel, key: ByteArray) {
    val path = model.nodePath(key)
    CardList {
        when {
            path == null -> CardRow(stringResource(R.string.no_path), stringResource(R.string.no_path_flood_note))
            path.isEmpty() -> CardRow(stringResource(R.string.hops_direct_title), stringResource(R.string.direct_note))
            else -> path.forEachIndexed { i, hop ->
                if (i > 0) CardDivider()
                CardRow(hop.display(), stringResource(R.string.hop_n, i + 1, hop.hash.uppercase()))
            }
        }
    }
}

/** Traces the path there and back, and shows each hop's SNR (§16.3). */
@Composable
private fun TraceCard(model: AppModel, key: ByteArray, error: (String) -> Unit) {
    val t = lichen
    val trace = model.trace(key)
    Column(Modifier.padding(top = 6.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        SecondaryButton(
            stringResource(if (trace?.state == TraceState.SENDING) R.string.tracing else R.string.trace_path),
            enabled = model.isConnected && trace?.state != TraceState.SENDING,
        ) { model.startTrace(key)?.let(error) }
        if (trace != null) {
            val you = stringResource(R.string.you)
            CardList {
                trace.legs.forEachIndexed { i, leg ->
                    if (i > 0) CardDivider()
                    CardRow("${leg.from?.display() ?: you} → ${leg.to?.display() ?: you}") {
                        val snr = leg.snrDb
                        Text(
                            snr?.let { "%.1f dB".format(it) } ?: "–",
                            fontFamily = FontFamily.Monospace,
                            color = when {
                                snr == null -> t.tx2
                                snr >= 0 -> t.ok
                                snr >= -7 -> t.warn
                                else -> t.sos
                            },
                        )
                    }
                }
            }
            Note(traceSummary(LocalResources.current, trace))
        }
    }
}

private fun traceSummary(res: android.content.res.Resources, t: TraceInfo): String = when (t.state) {
    TraceState.SENDING -> res.getString(R.string.trace_sending)
    TraceState.DONE -> res.getString(R.string.trace_done, "%.1f".format((t.rttMs ?: 0uL).toDouble() / 1000))
    TraceState.NO_ANSWER -> res.getString(R.string.trace_no_answer)
    TraceState.REFUSED -> res.getString(R.string.trace_refused)
}

/** Logging in to a room server or a repeater (§15.8, §16.5). */
@Composable
fun ServerLoginCard(model: AppModel, key: ByteArray, room: Boolean) {
    val t = lichen
    val res = LocalResources.current
    val info = model.server(key)
    val login = info?.login ?: ServerLogin.OUT
    var password by remember { mutableStateOf("") }
    var keep by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf<String?>(null) }
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        CardList {
            CardRow(stringResource(R.string.login), loginLabel(res, login))
            if (login.isIn) {
                if (info?.remembered == true) {
                    CardDivider()
                    CardRow(stringResource(R.string.password_kept), stringResource(R.string.password_kept_note))
                }
                CardDivider()
                Box(Modifier.clickable { model.serverLogout(key) }) {
                    CardRow(stringResource(R.string.log_out)) {}
                }
            } else {
                CardDivider()
                OutlinedTextField(
                    password, { password = it }, Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp),
                    singleLine = true,
                    placeholder = { Text(stringResource(if (room) R.string.room_password else R.string.server_password)) },
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false),
                    shape = RoundedCornerShape(12.dp), colors = fieldColors(),
                )
                if (room) {
                    CardDivider()
                    CardRow(stringResource(R.string.remember_password)) {
                        Switch(keep, { keep = it })
                    }
                }
                CardDivider()
                Box(Modifier.padding(12.dp)) {
                    SecondaryButton(
                        stringResource(if (login == ServerLogin.PENDING) R.string.login_pending else R.string.log_in),
                        enabled = model.isConnected && login != ServerLogin.PENDING && password.toByteArray().size <= 15,
                    ) { error = model.serverLogin(key, password, room && keep) }
                }
            }
        }
        Note(stringResource(if (room) R.string.room_login_note else R.string.repeater_login_note))
        error?.let { Text(it, fontSize = 13.sp, color = t.sos) }
    }
}

/** A repeater's status, neighbours and command line (§16.5). */
private fun LazyListScope.repeaterItems(model: AppModel, key: ByteArray, console: () -> Unit, error: (String) -> Unit) {
    item { SectionLabel(lichenString(R.string.status), Modifier.padding(top = 10.dp)) }
    item {
        val info = model.server(key)
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            CardList {
                info?.stats?.let {
                    StatusRows(it)
                    CardDivider()
                }
                Box(Modifier.padding(12.dp)) {
                    SecondaryButton(
                        stringResource(if (info?.statsPending == true) R.string.asking else R.string.read_status),
                        enabled = model.isConnected && info?.statsPending != true,
                    ) { model.requestServerStats(key)?.let(error) }
                }
            }
            if (info?.noAnswer == true) Text(stringResource(R.string.server_no_answer), fontSize = 13.sp, color = lichen.warnFg)
        }
    }
    item { SectionLabel(lichenString(R.string.neighbours), Modifier.padding(top = 10.dp)) }
    item {
        val info = model.server(key)
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            CardList {
                info?.neighbours?.let { ns ->
                    if (ns.isEmpty() && info?.neighboursPending != true) {
                        CardRow(stringResource(R.string.hears_nobody))
                        CardDivider()
                    }
                    ns.forEach { n ->
                        val ago = System.currentTimeMillis() / 1000 - n.heardAgoS.toLong()
                        CardRow(n.name ?: n.prefix.uppercase(), stringResource(R.string.heard_ago, model.heard(ago.toULong()))) {
                            Text("%.1f dB".format(n.snrDb), fontFamily = FontFamily.Monospace, color = lichen.tx2)
                        }
                        CardDivider()
                    }
                }
                Box(Modifier.padding(12.dp)) {
                    SecondaryButton(
                        stringResource(if (info?.neighboursPending == true) R.string.asking else R.string.ask_neighbours),
                        enabled = model.isConnected && info?.neighboursPending != true,
                    ) { model.requestNeighbours(key)?.let(error) }
                }
            }
            val ns = info?.neighbours
            if (info != null && ns != null && info.neighboursTotal.toInt() > ns.size) {
                Note(stringResource(R.string.neighbours_of, ns.size, info.neighboursTotal.toInt()))
            }
        }
    }
    item { SectionLabel(lichenString(R.string.command_line), Modifier.padding(top = 10.dp)) }
    item {
        CardList {
            Box(Modifier.clickable(onClick = console)) {
                CardRow(stringResource(R.string.console), stringResource(R.string.console_note)) { Chevron() }
            }
        }
    }
}

/** A string in a lazy list's scope (a section label). */
@Composable
private fun lichenString(id: Int): String = stringResource(id)

@Composable
private fun StatusRows(s: ServerStats) {
    val res = LocalResources.current
    CardRow(stringResource(R.string.battery), "%.2f V".format(s.batteryMv.toInt() / 1000.0))
    CardDivider()
    CardRow(stringResource(R.string.up_for), duration(res, s.uptimeS.toLong()))
    CardDivider()
    CardRow(stringResource(R.string.radio), stringResource(R.string.status_radio, s.noiseFloor.toInt(), s.lastRssi.toInt(), "%.1f".format(s.lastSnrDb)))
    CardDivider()
    CardRow(stringResource(R.string.packets), stringResource(R.string.status_packets, s.packetsRecv.toLong(), s.packetsSent.toLong(), s.txQueue.toInt()))
    CardDivider()
    CardRow(
        stringResource(R.string.airtime),
        listOfNotNull(
            stringResource(R.string.status_transmitting, duration(res, s.airTimeS.toLong())),
            s.rxAirTimeS?.let { stringResource(R.string.status_receiving, duration(res, it.toLong())) },
        ).joinToString(" · "),
    )
    CardDivider()
    CardRow(
        stringResource(R.string.flood_and_direct),
        stringResource(
            R.string.status_flood_direct,
            s.sentFlood.toLong(), s.sentDirect.toLong(), s.recvFlood.toLong(), s.recvDirect.toLong(),
            s.floodDups.toInt(), s.directDups.toInt(),
        ),
    )
    s.recvErrors?.let {
        CardDivider()
        CardRow(stringResource(R.string.receive_errors), it.toString())
    }
    s.posted?.let {
        CardDivider()
        CardRow(stringResource(R.string.posts), stringResource(R.string.status_posts, it.toInt(), s.postPushes?.toInt() ?: 0))
    }
}

private fun duration(res: android.content.res.Resources, s: Long): String = when {
    s >= 86_400 -> res.getString(R.string.duration_days, s / 86_400, s % 86_400 / 3_600)
    s >= 3_600 -> res.getString(R.string.duration_hours, s / 3_600, s % 3_600 / 60)
    else -> res.getString(R.string.duration_minutes, s / 60)
}

/** A repeater's command line: what was sent and what came back (§16.5). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ConsoleScreen(model: AppModel, key: ByteArray, back: () -> Unit) {
    val t = lichen
    val lines = model.server(key)?.console.orEmpty()
    var command by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    val state = rememberLazyListState()
    LaunchedEffect(lines.size) { if (lines.isNotEmpty()) state.animateScrollToItem(lines.size) }
    fun send() {
        val c = command.trim()
        if (c.isEmpty()) return
        error = model.sendServerCommand(key, c)
        if (error == null) command = ""
    }
    Column(Modifier.fillMaxSize().background(t.bg).imePadding()) {
        TopAppBar(
            navigationIcon = { IconButton(onClick = back) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) } },
            title = { Text(model.node(key)?.name ?: stringResource(R.string.console), fontSize = 22.sp) },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = t.bg, titleContentColor = t.tx, navigationIconContentColor = t.tx),
        )
        LazyColumn(
            Modifier.weight(1f).fillMaxWidth(),
            state = state,
            contentPadding = PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            item { Note(stringResource(R.string.console_hint)) }
            itemsIndexed(lines) { _, l ->
                Text(
                    if (l.outgoing) (if (l.text.isEmpty()) stringResource(R.string.console_refused) else "> " + l.text) else l.text,
                    fontFamily = FontFamily.Monospace, fontSize = 13.sp,
                    color = if (l.outgoing) t.lora else t.tx,
                )
            }
        }
        error?.let { Text(it, fontSize = 13.sp, color = t.sos, modifier = Modifier.padding(horizontal = 16.dp)) }
        Row(
            Modifier.fillMaxWidth().background(t.s1).windowInsetsPadding(WindowInsets.navigationBars).padding(12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            OutlinedTextField(
                command, { command = it }, Modifier.weight(1f), singleLine = true,
                placeholder = { Text(stringResource(R.string.command)) },
                textStyle = androidx.compose.ui.text.TextStyle(fontFamily = FontFamily.Monospace),
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false, imeAction = ImeAction.Send),
                keyboardActions = KeyboardActions(onSend = { send() }),
                shape = RoundedCornerShape(12.dp), colors = fieldColors(),
            )
            IconButton(onClick = { send() }, enabled = command.isNotBlank() && model.isConnected) {
                Icon(Icons.Filled.ArrowUpward, stringResource(R.string.send), tint = t.tx)
            }
        }
    }
}

// ----- saved radios (ADR-0042) -----

/** Settings › Radios: the radios this phone was paired with. */
@Composable
fun RadiosScreen(model: AppModel, back: () -> Unit, pair: () -> Unit) {
    val t = lichen
    var forgetting by remember { mutableStateOf<SavedRadio?>(null) }
    LaunchedEffect(Unit) { model.refreshSavedRadios() }
    ToolScreen(stringResource(R.string.radios), back) {
        if (model.savedRadios.isEmpty()) {
            item { Text(stringResource(R.string.radios_none), color = t.tx2) }
        } else {
            item {
                CardList {
                    model.savedRadios.forEachIndexed { i, r ->
                        if (i > 0) CardDivider()
                        RadioRow(model, r) { forgetting = r }
                    }
                }
            }
        }
        item {
            CardList {
                Box(Modifier.clickable(onClick = pair)) { CardRow(stringResource(R.string.pair_another_radio)) { Chevron() } }
            }
        }
        item { Note(stringResource(R.string.radios_history_note)) }
    }
    forgetting?.let { r ->
        AlertDialog(
            onDismissRequest = { forgetting = null },
            title = { Text(stringResource(R.string.forget_named, r.name)) },
            text = { Text(stringResource(R.string.forget_radio_note)) },
            confirmButton = {
                TextButton(onClick = {
                    model.forgetRadio(r)
                    forgetting = null
                }) { Text(stringResource(R.string.forget), color = t.sos) }
            },
            dismissButton = { TextButton(onClick = { forgetting = null }) { Text(stringResource(R.string.cancel), color = t.tx) } },
            containerColor = t.s1,
        )
    }
}

@Composable
private fun RadioRow(model: AppModel, r: SavedRadio, forget: () -> Unit) {
    val t = lichen
    var menu by remember { mutableStateOf(false) }
    val link = stringResource(
        when (r.link) {
            LinkKind.BLUETOOTH -> R.string.link_bluetooth
            LinkKind.USB -> R.string.link_usb
            LinkKind.TCP -> R.string.link_tcp
        },
    )
    val used = DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(r.lastUsedMs))
    Row(
        Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Box(Modifier.size(36.dp).background(t.s2, RoundedCornerShape(9.dp)), contentAlignment = Alignment.Center) {
            Icon(Icons.Outlined.SettingsInputAntenna, null, tint = if (r.current) t.lora else t.tx2)
        }
        Column(Modifier.weight(1f)) {
            Text(r.name, fontSize = 16.sp, color = t.tx)
            Text(stringResource(R.string.radio_last_used, link, used), fontSize = 13.sp, color = t.tx2)
        }
        if (r.current) Badge(stringResource(R.string.connected), t.okBg, t.okFg)
        Box {
            IconButton(onClick = { menu = true }) { Icon(Icons.Outlined.MoreVert, stringResource(R.string.radio_actions, r.name), tint = t.tx2) }
            DropdownMenu(menu, { menu = false }, containerColor = t.s1) {
                if (model.canSwitchTo(r)) {
                    DropdownMenuItem(text = { Text(stringResource(R.string.connect)) }, onClick = {
                        menu = false
                        model.switchRadio(r)
                    })
                }
                DropdownMenuItem(text = { Text(stringResource(R.string.forget), color = t.sos) }, onClick = {
                    menu = false
                    forget()
                })
            }
        }
    }
}

// ----- channels by QR code (protocol.md §15.3) -----

/** A channel's QR code in the stock app's format, after a warning. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChannelQrSheet(model: AppModel, channel: MeshChannel, dismiss: () -> Unit) {
    val t = lichen
    var shown by remember { mutableStateOf(false) }
    ModalBottomSheet(onDismissRequest = dismiss, sheetState = rememberModalBottomSheetState(true), containerColor = t.bg) {
        Column(
            Modifier.padding(horizontal = 16.dp).padding(bottom = 24.dp).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(14.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(stringResource(R.string.share_channel), fontSize = 22.sp, color = t.tx, modifier = Modifier.fillMaxWidth())
            Banner(BannerKind.WARN, stringResource(R.string.whoever_scans_joins), stringResource(R.string.channel_qr_warning, channel.name))
            val text = if (shown) model.channelQr(channel.id) else null
            if (text != null) {
                QrImage(text, stringResource(R.string.qr_of, channel.name), Modifier.widthIn(max = 280.dp).fillMaxWidth())
                Text(channel.name, fontSize = 17.sp, color = t.tx)
            } else {
                PrimaryButton(stringResource(R.string.show_code)) { shown = true }
            }
        }
    }
}

/** Scans a channel's QR code from a MeshCore app and joins it after confirmation. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ScanChannelSheet(model: AppModel, dismiss: () -> Unit, open: (ChatId) -> Unit) {
    val t = lichen
    val res = LocalResources.current
    var code by remember { mutableStateOf<ChannelCode?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    ModalBottomSheet(onDismissRequest = dismiss, sheetState = rememberModalBottomSheetState(true), containerColor = t.bg) {
        Column(
            Modifier.padding(horizontal = 16.dp).padding(bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Text(stringResource(R.string.scan_channel), fontSize = 22.sp, color = t.tx)
            val c = code
            if (c != null) {
                CardList { CardRow(c.name, channelKindLabel(res, c.kind)) }
                Note(stringResource(if (c.joined) R.string.channel_already_followed else R.string.channel_will_follow))
                PrimaryButton(stringResource(if (c.joined) R.string.open else R.string.join_the_channel)) {
                    model.joinChannel(ChannelKind.PRIVATE_CHANNEL, c.name, c.secretHex)
                        .onSuccess { dismiss(); open(it) }
                        .onFailure { error = it.message }
                }
            } else {
                QrScanner(
                    found = { text ->
                        val parsed = model.parseChannelQr(text)
                        if (parsed != null) code = parsed else error = res.getString(R.string.not_a_channel_code)
                    },
                    modifier = Modifier.fillMaxWidth().heightIn(max = 360.dp).background(Color.Black, CardShape),
                    prefixes = listOf("meshcore://channel/"),
                )
                Note(stringResource(R.string.scan_channel_hint))
            }
            error?.let { Banner(BannerKind.SOS, it) }
        }
    }
}
