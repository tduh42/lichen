package com.tduhamel.lichen.ui

import androidx.compose.ui.platform.LocalDensity
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import com.tduhamel.lichen.core.SignalLimits
import com.tduhamel.lichen.core.LichenException
import com.tduhamel.lichen.core.NodeReading
import com.tduhamel.lichen.core.ReadingKind
import com.tduhamel.lichen.core.ReadingValue
import com.tduhamel.lichen.core.ReadingsState
import com.tduhamel.lichen.model.askReadings
import com.tduhamel.lichen.model.hex
import com.tduhamel.lichen.model.nodeReadings
import com.tduhamel.lichen.model.nodeReadingsState
import kotlinx.coroutines.launch
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.semantics.SemanticsPropertyKey
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import com.tduhamel.lichen.core.NodeRoute
import com.tduhamel.lichen.core.RouteLeg
import com.tduhamel.lichen.core.RouteStop
import com.tduhamel.lichen.core.SignalRating
import com.tduhamel.lichen.core.SignalReading
import com.tduhamel.lichen.model.dbText
import com.tduhamel.lichen.model.distanceBearing
import com.tduhamel.lichen.model.NodeFilter
import com.tduhamel.lichen.model.TWO_HOURS_S
import com.tduhamel.lichen.model.heardWithin
import com.tduhamel.lichen.model.initials
import com.tduhamel.lichen.model.nodePlace
import com.tduhamel.lichen.model.nodeRoute
import com.tduhamel.lichen.model.nodeSections
import com.tduhamel.lichen.model.nodeSignalHistory
import com.tduhamel.lichen.model.showNodeOnMap
import com.tduhamel.lichen.model.wallNow
import kotlin.math.abs
import kotlin.math.ceil
import kotlin.math.floor
import kotlinx.coroutines.delay
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
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.layout.wrapContentWidth
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
import com.tduhamel.lichen.core.SavedRadio
import com.tduhamel.lichen.core.ServerLogin
import com.tduhamel.lichen.core.ServerStats
import com.tduhamel.lichen.core.TraceState
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.canSwitchTo
import com.tduhamel.lichen.model.channelQr
import com.tduhamel.lichen.model.display
import com.tduhamel.lichen.model.duration
import com.tduhamel.lichen.model.forgetRadio
import com.tduhamel.lichen.model.heard
import com.tduhamel.lichen.model.node
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

/** A room chat's subtitle: what it is, and the login (§15.8). */
@Composable
fun roomSubtitle(model: AppModel, chat: ChatId): String {
    val login = model.server(chat.hex.unhex())?.login ?: ServerLogin.OUT
    return stringResource(R.string.meshcore_room_login, loginLabel(LocalResources.current, login))
}

/** A screen of the mesh tools: a title bar over a list, with a back arrow unless it is a tab's
 * root (the Nodes tab). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ToolScreen(
    title: String,
    back: (() -> Unit)?,
    state: androidx.compose.foundation.lazy.LazyListState = rememberLazyListState(),
    content: LazyListScope.() -> Unit,
) {
    val t = lichen
    Column(Modifier.fillMaxSize().background(t.bg)) {
        TopAppBar(
            navigationIcon = {
                if (back != null) IconButton(onClick = back) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) }
            },
            title = { Text(title, fontSize = 22.sp) },
            colors = TopAppBarDefaults.topAppBarColors(containerColor = t.bg, titleContentColor = t.tx, navigationIconContentColor = t.tx),
        )
        LazyColumn(
            // A tab's root (no way back): the tab bar below takes the navigation bar.
            Modifier.weight(1f).then(if (back != null) Modifier.windowInsetsPadding(WindowInsets.navigationBars) else Modifier),
            state = state,
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

// ----- nodes (§16.1, ADR-0049) -----

/** The filter chips over a list of nodes (the Nodes tab, the Map's layers): All, People,
 * Repeaters, Rooms with how many there are, Heard in 2 h; in one scrolling line, or wrapped
 * onto more ([wrap], in a menu). */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun NodeFilterChips(model: AppModel, nowS: Long, selected: NodeFilter, wrap: Boolean = false, choose: (NodeFilter) -> Unit) {
    val t = lichen
    val chips = @Composable {
        NodeFilter.entries.forEach { f ->
            val count = if (f == NodeFilter.HEARD) null else model.nodes.count { f.keeps(it, nowS) }
            FilterChip(
                selected = selected == f,
                onClick = { choose(f) },
                label = { Text(stringResource(f.label) + (count?.let { " $it" } ?: "")) },
                colors = FilterChipDefaults.filterChipColors(
                    containerColor = t.s1, labelColor = t.tx,
                    selectedContainerColor = t.actBg, selectedLabelColor = t.actFg,
                ),
            )
        }
    }
    if (wrap) {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) { chips() }
    } else {
        Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) { chips() }
    }
}

/** Now, Unix seconds, read again every 30 s: "heard" times and the 2 h and 24 h limits move on. */
@Composable
fun nowSeconds(): Long {
    var tick by remember { mutableLongStateOf(0L) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(30_000)
            tick++
        }
    }
    @Suppress("UNUSED_EXPRESSION") tick
    return wallNow() / 1000
}

/** The phone's position while [who] is on screen: each node's distance and bearing. */
@Composable
private fun UsesLocation(model: AppModel, who: String) {
    DisposableEffect(Unit) {
        model.needsLocation(who, true)
        onDispose { model.needsLocation(who, false) }
    }
}

/** The Nodes tab (mockup `Main`): who the radio hears directly and how well, then those it
 * reaches through repeaters. */
@Composable
fun NodesScreen(model: AppModel, open: (ByteArray) -> Unit) {
    val t = lichen
    var filter by remember { mutableStateOf(NodeFilter.ALL) }
    var query by remember { mutableStateOf("") }
    UsesLocation(model, "nodes")
    val nowS = nowSeconds()
    val shown = model.nodes.filter { filter.keeps(it, nowS) && (query.isBlank() || it.name.contains(query.trim(), ignoreCase = true)) }
    val (reach, through) = nodeSections(shown, nowS)
    ToolScreen(stringResource(R.string.tab_nodes), back = null) {
        item {
            val heard = model.nodes.count { it.heardWithin(nowS, TWO_HOURS_S) }
            Text(
                pluralStringResource(R.plurals.nodes_known, model.nodes.size, model.nodes.size) + " · " +
                    pluralStringResource(R.plurals.nodes_heard_2h, heard, heard),
                fontSize = 13.sp, color = t.tx2, modifier = Modifier.padding(horizontal = 4.dp),
            )
        }
        item { NodeFilterChips(model, nowS, filter) { filter = it } }
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
            if (reach.isNotEmpty()) {
                item { SectionLabel(stringResource(R.string.nodes_in_reach), Modifier.padding(top = 10.dp)) }
                item {
                    CardList {
                        reach.forEachIndexed { i, n ->
                            if (i > 0) CardDivider(inset = 68.dp)
                            NodeRow(model, n, nowS, inReach = true) { open(n.key) }
                        }
                    }
                }
            }
            if (through.isNotEmpty()) {
                item { SectionLabel(stringResource(R.string.nodes_through_repeaters), Modifier.padding(top = 10.dp)) }
                item {
                    CardList {
                        through.forEachIndexed { i, n ->
                            if (i > 0) CardDivider(inset = 68.dp)
                            NodeRow(model, n, nowS, inReach = false) { open(n.key) }
                        }
                    }
                }
            }
            item { Note(stringResource(R.string.nodes_note)) }
        }
    }
}

/** What a node is, in a word: its kind; "MeshCore app" for someone with the stock app; nothing
 * for a lichen user (the badge says it). */
@Composable
private fun nodeKindWord(n: NodeInfo): String? = when {
    n.kind == NodeKind.COMPANION && n.lichen -> null
    n.kind == NodeKind.COMPANION -> stringResource(R.string.node_kind_stock)
    else -> nodeKindLabel(LocalResources.current, n.kind)
}

/** A person's initials, or a node's kind on its tile (a repeater's in `lora`). */
@Composable
private fun NodeLeading(n: NodeInfo, size: androidx.compose.ui.unit.Dp) {
    val t = lichen
    if (n.kind == NodeKind.COMPANION) {
        Avatar(n.name, n.key, size)
        return
    }
    val repeater = n.kind == NodeKind.REPEATER
    Box(
        Modifier.size(size).background(if (repeater) t.loraBg else t.s2, RoundedCornerShape(size * 0.27f)),
        contentAlignment = Alignment.Center,
    ) {
        val tint = if (repeater) t.loraFg else t.tx
        if (repeater) {
            Icon(painterResource(R.drawable.ic_radio_tower), null, tint = tint, modifier = Modifier.size(size * 0.55f))
        } else {
            Icon(nodeKindIcon(n.kind), null, tint = tint, modifier = Modifier.size(size * 0.55f))
        }
    }
}

/** The names of the repeaters a route goes through ("hub, crest"), when its path names them. */
private fun viaNames(route: NodeRoute?): String? =
    route?.stops?.drop(1)?.dropLast(1)?.map { it.names.joinToString(" / ") }?.filter { it.isNotEmpty() }?.takeIf { it.isNotEmpty() }?.joinToString(", ")

/** A stop's name: its names joined (a hash several repeaters share), else its hash. */
private fun stopName(s: RouteStop): String = s.names.joinToString(" / ").ifEmpty { s.hash?.uppercase() ?: "?" }

@Composable
private fun NodeRow(model: AppModel, n: NodeInfo, nowS: Long, inReach: Boolean, open: () -> Unit) {
    val t = lichen
    val route = if (inReach) null else model.nodeRoute(n.key)
    val recent = n.heardWithin(nowS, TWO_HOURS_S)
    val parts = listOfNotNull(
        nodeKindWord(n),
        viaNames(route)?.let { stringResource(R.string.via, it) },
        model.distanceBearing(n.lat, n.lon),
    )
    val heard = model.heard(n.lastHeardS)
    Row(
        Modifier.fillMaxWidth().clickable(onClick = open).heightIn(min = 48.dp).padding(horizontal = 16.dp, vertical = 11.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        NodeLeading(n, 40.dp)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(
                    n.name, fontSize = 16.sp, fontWeight = FontWeight.SemiBold, color = t.tx, maxLines = 1,
                    overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false),
                )
                if (n.lichen) LichenBadge()
                if (!n.onRadio) Badge(stringResource(R.string.heard_badge), t.s2, t.tx2)
            }
            Text(
                buildAnnotatedString {
                    parts.forEach { append(it); append(" · ") }
                    // Heard lately: in `ok`.
                    if (recent) withStyle(SpanStyle(color = t.ok)) { append(heard) } else append(heard)
                },
                fontSize = 13.sp, color = t.tx2,
            )
        }
        if (inReach) SignalMeter(n.signal, n.snrDb) else HopPill(n.hops)
    }
}

/** One node (mockup `NodePage`): who it is and how well it is heard, how it reaches us (the
 * route, a trace), its signal over a day or a week; a room's login, a repeater's administration
 * (§16). */
@Composable
fun NodeScreen(model: AppModel, key: ByteArray, back: () -> Unit, console: () -> Unit, openChat: (ChatId) -> Unit) {
    val t = lichen
    val n = model.node(key)
    var error by remember { mutableStateOf<String?>(null) }
    UsesLocation(model, "node")
    val nowS = nowSeconds()
    val list = rememberLazyListState()
    val scope = rememberCoroutineScope()
    val password = remember { FocusRequester() }
    // Readings' Log in: the list goes to the login card below (its index, counted as the items
    // are laid out below) and its password field takes the keyboard.
    val readings = n != null && !key.contentEquals(model.nodeKey)
    val loginIndex = (if (n != null) 1 else 0) + 2 + (if (n != null) 2 else 0) + (if (readings) 1 else 0) +
        (if (n?.kind == NodeKind.ROOM_SERVER) 2 else 1)
    val toLogin: () -> Unit = {
        scope.launch {
            list.animateScrollToItem(loginIndex)
            withFrameNanos { }
            runCatching { password.requestFocus() }
        }
    }
    ToolScreen(n?.name ?: stringResource(R.string.node_kind_unknown), back, list) {
        if (n != null) item { NodeHeader(model, n, nowS) }
        item { SectionLabel(stringResource(R.string.how_it_reaches_you), Modifier.padding(top = 10.dp)) }
        item { RouteCard(model, key) { error = it } }
        if (n != null) item { SignalSection(model, n, nowS) }
        // What it sends: never for our own radio (Radio settings has its battery).
        if (n != null && readings) item { ReadingsCard(model, n, nowS, toLogin) }
        if (n != null) item {
            CardList(Modifier.padding(top = 10.dp)) {
                if (!n.onRadio) {
                    CardRow(stringResource(R.string.heard_not_added))
                    CardDivider()
                }
                if (n.lat != null && n.lon != null) {
                    CardRow(stringResource(R.string.advertised_position), "%.5f, %.5f".format(n.lat, n.lon))
                    CardDivider()
                }
                MeshCoreKeyRow(key)
            }
        }
        if (n?.kind == NodeKind.ROOM_SERVER) {
            item { SectionLabel(stringResource(R.string.room), Modifier.padding(top = 10.dp)) }
            item {
                CardList {
                    Box(Modifier.clickable { openChat(ChatId.room(key)) }) {
                        CardRow(stringResource(R.string.open_room), stringResource(R.string.open_room_note), merge = false) { Chevron() }
                    }
                }
            }
            item { ServerLoginCard(model, key, room = true, password) }
        }
        if (n?.kind == NodeKind.REPEATER) {
            item { SectionLabel(stringResource(R.string.administration), Modifier.padding(top = 10.dp)) }
            item { ServerLoginCard(model, key, room = false, password) }
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

/** The node on top: its tile, name, kind and distance, when last heard; its signal (heard
 * directly) or how many repeaters away. */
@Composable
private fun NodeHeader(model: AppModel, n: NodeInfo, nowS: Long) {
    val t = lichen
    Row(
        Modifier.fillMaxWidth().background(t.s1, CardShape).padding(horizontal = 16.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        NodeLeading(n, 56.dp)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(n.name, fontSize = 20.sp, fontWeight = FontWeight.Bold, color = t.tx, modifier = Modifier.weight(1f, fill = false))
                if (n.lichen) LichenBadge()
            }
            val what = listOfNotNull(nodeKindWord(n), model.distanceBearing(n.lat, n.lon)).joinToString(" · ")
            if (what.isNotEmpty()) Text(what, fontSize = 13.sp, color = t.tx2)
            Text(
                stringResource(R.string.heard_cap, model.heard(n.lastHeardS)),
                fontSize = 13.sp, fontWeight = FontWeight.SemiBold,
                color = if (n.heardWithin(nowS, TWO_HOURS_S)) t.ok else t.tx2,
            )
        }
        if (n.signal != SignalRating.NONE) SignalMeter(n.signal, n.snrDb, large = true) else HopPill(n.hops)
    }
}

/** How it reaches us (§16.3): the route stop by stop, each leg's last signal; the last trace,
 * Trace again, Show on the map. */
@Composable
private fun RouteCard(model: AppModel, key: ByteArray, error: (String) -> Unit) {
    val t = lichen
    val route = model.nodeRoute(key)
    val trace = model.trace(key)
    Column(
        Modifier.fillMaxWidth().background(t.s1, CardShape).padding(horizontal = 16.dp, vertical = 14.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        if (route != null && route.stops.size >= 2) {
            RouteRibbon(model, route)
        } else {
            Text(stringResource(R.string.no_path), fontSize = 15.sp, color = t.tx)
        }
        // When the legs were traced, and today's trace's figures.
        val tracedAt = route?.legs?.filter { it.traced }?.mapNotNull { it.atS }?.maxOrNull()
        Text(
            when (trace?.state) {
                TraceState.SENDING -> stringResource(R.string.trace_sending)
                TraceState.NO_ANSWER -> stringResource(R.string.trace_no_answer)
                TraceState.REFUSED -> stringResource(R.string.trace_refused)
                TraceState.DONE -> stringResource(
                    R.string.traced_summary,
                    model.heard(tracedAt ?: (wallNow() / 1000).toULong()),
                    "%.1f".format((trace.rttMs ?: 0uL).toDouble() / 1000),
                    "%.1f".format(trace.airtimeMs.toDouble() / 1000),
                )
                null -> tracedAt?.let { stringResource(R.string.traced_at, model.heard(it)) } ?: stringResource(R.string.not_traced)
            },
            fontSize = 13.sp, color = t.tx2,
        )
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            PrimaryButton(
                stringResource(
                    when {
                        trace?.state == TraceState.SENDING -> R.string.tracing
                        trace != null || tracedAt != null -> R.string.trace_again
                        else -> R.string.trace_path
                    },
                ),
                Modifier.weight(1f),
                enabled = model.isConnected && trace?.state != TraceState.SENDING,
            ) { model.startTrace(key)?.let(error) }
            // Where the node, or a repeater on the way, advertises a position.
            if (model.nodePlace(key) != null) {
                SecondaryButton(stringResource(R.string.show_on_map), Modifier.weight(1f)) { model.showNodeOnMap(key) }
            }
        }
    }
}

/** You → each repeater → the node, a leg between each two: its colour by rating and its SNR,
 * or a dashed `line` "not measured". */
@Composable
private fun RouteRibbon(model: AppModel, route: NodeRoute) {
    val you = stringResource(R.string.you)
    val last = route.stops.lastIndex
    val names = route.stops.mapIndexed { i, s -> if (i == 0) you else stopName(s) }
    // Five stops fit a phone's width; more scroll.
    val scrolls = route.stops.size > 5
    Row(
        Modifier.fillMaxWidth().then(if (scrolls) Modifier.horizontalScroll(rememberScrollState()) else Modifier),
        verticalAlignment = Alignment.Top,
    ) {
        route.stops.forEachIndexed { i, stop ->
            RouteStopView(model, stop, i, last, names[i])
            if (i < last) {
                val leg = route.legs.getOrNull(i)
                RouteLegView(leg, names[i], names[i + 1], if (scrolls) Modifier.width(64.dp) else Modifier.weight(1f))
            }
        }
    }
}

@Composable
private fun RouteStopView(model: AppModel, stop: RouteStop, i: Int, last: Int, name: String) {
    val t = lichen
    Column(Modifier.width(62.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(5.dp)) {
        val node = stop.key?.let { model.node(it) }
        // Every tile in the same 40 dp: the node's ring fits in it, the names line up.
        Box(Modifier.size(40.dp), contentAlignment = Alignment.Center) {
            when {
                // Our radio: its initials on a dark disc.
                i == 0 -> Box(Modifier.size(34.dp).background(t.actBg, CircleShape), contentAlignment = Alignment.Center) {
                    // Sized by the disc, not the font scale: larger text would not fit it.
                    Text(
                        initials(model.nodeName.ifEmpty { name }), fontSize = with(LocalDensity.current) { 12.dp.toSp() },
                        fontWeight = FontWeight.Bold, color = t.actFg, maxLines = 1,
                    )
                }
                i == last && node != null -> Box(Modifier.border(2.dp, t.tx2, if (node.kind == NodeKind.COMPANION) CircleShape else RoundedCornerShape(11.dp)).padding(3.dp)) {
                    NodeLeading(node, 30.dp)
                }
                else -> Box(Modifier.size(34.dp).background(t.loraBg, RoundedCornerShape(9.dp)), contentAlignment = Alignment.Center) {
                    Icon(painterResource(R.drawable.ic_radio_tower), null, tint = t.loraFg, modifier = Modifier.size(19.dp))
                }
            }
        }
        // Their own line heights: the theme's (24 sp) left the caption far below the name.
        Text(
            name, fontSize = 12.sp, lineHeight = 15.sp, fontWeight = FontWeight.SemiBold, color = t.tx, maxLines = 2,
            overflow = TextOverflow.Ellipsis, textAlign = TextAlign.Center,
        )
        if (i == 0 && model.nodeName.isNotEmpty()) {
            // On one line, centred under its stop: past the stop's width (into the leg beside it,
            // whose line and figure sit higher) rather than wrapped. Up to 1.3 times larger,
            // within the card's margin either side of its stop; on a second line only when even
            // that is too narrow.
            CappedFontScale(1.3f) {
                Text(
                    model.nodeName,
                    // Just under the name, closer than the column's spacing.
                    Modifier.offset(y = (-3).dp).wrapContentWidth(unbounded = true).widthIn(max = 92.dp),
                    fontSize = 10.sp, lineHeight = 13.sp, color = t.tx2, maxLines = 2, overflow = TextOverflow.Ellipsis,
                    textAlign = TextAlign.Center,
                )
            }
        }
    }
}

@Composable
private fun RouteLegView(leg: RouteLeg?, from: String, to: String, modifier: Modifier) {
    val t = lichen
    val snr = leg?.snrDb
    val rating = leg?.signal ?: SignalRating.NONE
    val label = snr?.let { dbText(it) } ?: stringResource(R.string.not_measured)
    val said = stringResource(R.string.leg_said, from, to, label)
    Column(
        // The line at the tiles' middle.
        modifier.padding(top = 18.dp).clearAndSetSemantics { contentDescription = said },
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        if (snr != null) {
            Box(Modifier.fillMaxWidth().height(4.dp).background(if (rating != SignalRating.NONE) signalColor(rating) else t.tx2, CircleShape))
        } else {
            // Nothing measured it: dashed, in `line`.
            Canvas(Modifier.fillMaxWidth().height(4.dp)) {
                drawLine(
                    t.line, Offset(0f, size.height / 2), Offset(size.width, size.height / 2), strokeWidth = size.height,
                    cap = StrokeCap.Round, pathEffect = PathEffect.dashPathEffect(floatArrayOf(8.dp.toPx(), 5.dp.toPx())),
                )
            }
        }
        // Up to 1.3 times larger in the leg's narrow room, on two lines rather than cut ("not
        // measured"; a word wider than the leg would be).
        CappedFontScale(1.3f) {
            Text(
                label, fontSize = if (snr != null) 12.sp else 11.sp, fontWeight = if (snr != null) FontWeight.SemiBold else FontWeight.Normal,
                color = if (snr != null && rating != SignalRating.NONE) signalColor(rating) else t.tx2,
                style = TextStyle(fontFeatureSettings = "tnum"), maxLines = 2, textAlign = TextAlign.Center,
            )
        }
    }
}

/** Its signal over the last 24 h or 7 days (ADR-0049); for a node heard only through
 * repeaters, why there is none. */
@Composable
private fun SignalSection(model: AppModel, n: NodeInfo, nowS: Long) {
    val t = lichen
    var week by rememberSaveable { mutableStateOf(false) }
    val history = model.nodeSignalHistory(n.key)
    Column(Modifier.padding(top = 10.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            SectionLabel(stringResource(if (week) R.string.signal_7d else R.string.signal_24h), Modifier.weight(1f))
            if (history.isNotEmpty()) {
                SegmentedTabs(listOf(stringResource(R.string.range_24h), stringResource(R.string.range_7d)), if (week) 1 else 0) { week = it == 1 }
            }
        }
        Column(Modifier.fillMaxWidth().background(t.s1, CardShape).padding(horizontal = 16.dp, vertical = 12.dp)) {
            if (history.isEmpty()) {
                Text(stringResource(R.string.no_own_signal), fontSize = 14.sp, color = t.tx2)
                return@Column
            }
            val window = if (week) 7 * 86_400L else 86_400L
            val points = history.filter { nowS - it.atS.toLong() <= window }
            if (points.isEmpty()) {
                Text(stringResource(R.string.no_reading_window), fontSize = 14.sp, color = t.tx2)
            } else {
                SignalChart(points, nowS, window, model.signalLimits)
            }
            val now = n.snrDb ?: history.last().snrDb
            Row(Modifier.fillMaxWidth().padding(top = 6.dp)) {
                Text(
                    stringResource(if (week) R.string.chart_since_7d else R.string.chart_since_24h), fontSize = 11.sp, color = t.tx2,
                    modifier = Modifier.weight(1f).alignByBaseline(),
                )
                Text(
                    stringResource(R.string.signal_now, dbText(now)), color = t.tx2, modifier = Modifier.alignByBaseline(),
                    style = TextStyle(fontSize = 11.sp, fontFeatureSettings = "tnum"),
                )
            }
        }
    }
}

/** The SNR limits the signal chart drew its bands at (Good, Fair), and its dB range (low,
 * high): for the tests, which check them against the core's. */
val ChartLimitsKey = SemanticsPropertyKey<Pair<Float, Float>>("ChartLimits")
val ChartRangeKey = SemanticsPropertyKey<Pair<Float, Float>>("ChartRange")

/** The readings in their window: a dot each, coloured by its rating, joined by a thin line; dB
 * up the side. Behind them, faint bands at the preset's limits (ADR-0049): Good above the first,
 * Fair down to the second, weak below; the dB range takes both in. None before a radio. */
@Composable
private fun SignalChart(points: List<SignalReading>, nowS: Long, window: Long, limits: SignalLimits?) {
    val t = lichen
    val measurer = rememberTextMeasurer()
    val said = pluralStringResource(R.plurals.chart_said, points.size, points.size, dbText(points.last().snrDb)) +
        (limits?.let { ", " + stringResource(R.string.chart_limits, dbText(it.goodSnrDb), dbText(it.fairSnrDb)) } ?: "")
    val goodLabel = stringResource(R.string.chart_good)
    val fairLabel = stringResource(R.string.chart_fair)
    val colors = points.map { if (it.signal == SignalRating.NONE) t.tx2 else signalColor(it.signal) }
    val values = points.map { it.snrDb } + listOfNotNull(limits?.goodSnrDb, limits?.fairSnrDb)
    val lo = floor(minOf(values.min() - 1f, values.max() - 6f))
    // Room above the Good limit for its label, however low the readings.
    val hi = ceil(maxOf(values.max() + 1f, lo + 6f, (limits?.goodSnrDb ?: Float.NEGATIVE_INFINITY) + 3f))
    val labelStyle = TextStyle(fontSize = 10.sp, color = t.tx2, fontFeatureSettings = "tnum")
    // Taller with larger text, so that its labels keep their room.
    val tall = 92.dp * LocalDensity.current.fontScale.coerceIn(1f, 1.5f)
    Canvas(
        Modifier.fillMaxWidth().height(tall).clearAndSetSemantics {
            contentDescription = said
            set(ChartRangeKey, lo to hi)
            if (limits != null) set(ChartLimitsKey, limits.goodSnrDb to limits.fairSnrDb)
        },
    ) {
        val h = size.height
        fun y(db: Float) = h - (db - lo) / (hi - lo) * h
        // dB up the side, with faint lines: at the limits when there are some (then the top and
        // the bottom where they leave room), else the top, middle and bottom; as many as their
        // labels leave room for, the side as wide as the widest.
        val ticks = mutableListOf<Float>()
        val room = measurer.measure("0", labelStyle).size.height + 2.dp.toPx()
        val wanted = if (limits != null) listOf(limits.goodSnrDb, limits.fairSnrDb, hi, lo) else listOf(hi, (hi + lo) / 2, lo)
        for (db in wanted) if (ticks.none { abs(y(it) - y(db)) < room }) ticks += db
        val tickLabels = ticks.map { it to measurer.measure(dbText(it).replace(" dB", ""), labelStyle) }
        val left = maxOf(34.dp.toPx(), (tickLabels.maxOfOrNull { it.second.size.width } ?: 0) + 6.dp.toPx())
        val w = size.width - left
        fun x(at: ULong) = left + ((at.toLong() - (nowS - window)).toFloat() / window) * w
        // The bands first, faint: the dots stay the main thing.
        if (limits != null) {
            val good = y(limits.goodSnrDb).coerceIn(0f, h)
            val fair = y(limits.fairSnrDb).coerceIn(good, h)
            drawRect(t.okBg.copy(alpha = BAND_ALPHA), Offset(left, 0f), Size(w, good))
            drawRect(t.warnBg.copy(alpha = BAND_ALPHA), Offset(left, good), Size(w, fair - good))
            drawRect(t.sosBg.copy(alpha = BAND_ALPHA), Offset(left, fair), Size(w, h - fair))
            // Each limit named at its left end, just above it.
            val pad = 4.dp.toPx()
            for ((text, at) in listOf(goodLabel to good, fairLabel to fair)) {
                val l = measurer.measure(text, labelStyle)
                drawText(l, topLeft = Offset(left + pad, (at - l.size.height).coerceIn(0f, h - l.size.height)))
            }
        }
        for ((db, text) in tickLabels) {
            val yy = y(db).coerceIn(0f, h)
            drawLine(t.line, Offset(left, yy), Offset(size.width, yy), strokeWidth = 1.dp.toPx())
            drawText(text, topLeft = Offset(0f, (yy - text.size.height / 2).coerceIn(0f, h - text.size.height)))
        }
        val pts = points.map { Offset(x(it.atS).coerceIn(left, size.width), y(it.snrDb)) }
        if (pts.size > 1) {
            val path = Path().apply {
                moveTo(pts.first().x, pts.first().y)
                pts.drop(1).forEach { lineTo(it.x, it.y) }
            }
            drawPath(path, t.tx2.copy(alpha = 0.5f), style = Stroke(1.5.dp.toPx()))
        }
        pts.forEachIndexed { i, p -> drawCircle(colors[i], 3.5.dp.toPx(), p) }
    }
}

// ----- a node's readings (§16.7, ADR-0050) -----

/** "Readings it sends" (mockup `NodePage`): its battery, its temperature and whatever else it
 * sent, when it sent them, and Ask again; or why there is nothing yet. Only the buttons ask:
 * opening the page never does. A repeater or a room answers only those logged in: Log in takes
 * the user to its login card. */
@Composable
private fun ReadingsCard(model: AppModel, n: NodeInfo, nowS: Long, toLogin: () -> Unit) {
    val t = lichen
    val res = LocalResources.current
    val readings = model.nodeReadings(n.key)
    val state = model.nodeReadingsState(n.key)
    val refusal = model.readingsRefusals[n.key.hex()]
    val last = readings.lastOrNull()
    val server = n.kind == NodeKind.REPEATER || n.kind == NodeKind.ROOM_SERVER
    val loggedIn = !server || (model.server(n.key)?.login ?: ServerLogin.OUT).isIn
    val refused = state == ReadingsState.REFUSED || refusal is LichenException.UnknownContact
    val loginLine = stringResource(if (n.kind == NodeKind.ROOM_SERVER) R.string.readings_login_room else R.string.readings_login_repeater)
    // What became of the last request, until the next one.
    val outcome = when {
        refused -> stringResource(R.string.readings_refused)
        refusal != null -> refusal.display(res)
        state == ReadingsState.ANSWERED_EMPTY -> stringResource(R.string.readings_empty)
        state == ReadingsState.NO_ANSWER -> stringResource(if (server) R.string.readings_no_answer_server else R.string.readings_no_answer_person)
        else -> null
    }
    val action: @Composable () -> Unit = {
        when {
            refused -> Unit
            state == ReadingsState.ASKING -> Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                CircularProgressIndicator(Modifier.size(16.dp), color = t.tx2, strokeWidth = 2.dp)
                Text(stringResource(R.string.asking), fontSize = 14.sp, color = t.tx2)
            }
            !loggedIn -> ReadingsButton(stringResource(R.string.log_in), toLogin)
            else -> ReadingsButton(stringResource(if (last != null) R.string.readings_ask_again else R.string.readings_ask)) { model.askReadings(n.key) }
        }
    }
    Column(Modifier.padding(top = 10.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        SectionLabel(stringResource(R.string.readings_title))
        CardList {
            if (last == null) {
                // Nothing yet: what to do, or why it cannot be done.
                SideOrUnder(
                    Modifier.fillMaxWidth().heightIn(min = 48.dp).padding(horizontal = 16.dp, vertical = 10.dp),
                    text = {
                        Text(
                            outcome ?: if (loggedIn) stringResource(R.string.readings_none) else loginLine,
                            fontSize = 15.sp, color = if (outcome != null) t.tx2 else t.tx,
                        )
                    },
                    action = action,
                )
                return@CardList
            }
            last.batteryMv?.let { mv ->
                val volts = "%.2f".format(mv.toDouble() / 1000)
                ReadingRow(
                    stringResource(R.string.battery),
                    last.batteryPercent?.let { stringResource(R.string.readings_battery_value, volts, it.toInt()) } ?: "$volts V",
                )
                CardDivider()
            }
            last.temperatureC?.let {
                ReadingRow(stringResource(R.string.readings_temperature), "${decimals(it, 1)} °C")
                CardDivider()
            }
            last.values.forEach { v ->
                val label = readingLabel(v.kind)
                ReadingRow(if (v.channel.toInt() == 1) label else stringResource(R.string.readings_sensor, label, v.channel.toInt()), readingValue(v))
                CardDivider()
            }
            // Its battery over the week, once there are two to join.
            val battery = readings.filter { it.batteryMv != null && nowS - it.atS.toLong() <= 7 * 86_400L }
            if (battery.size >= 2) {
                BatteryChart(battery, nowS)
                CardDivider()
            }
            SideOrUnder(
                Modifier.fillMaxWidth().heightIn(min = 48.dp).padding(horizontal = 16.dp, vertical = 6.dp),
                text = { Text(stringResource(R.string.readings_sent, model.heard(last.atS)), fontSize = 15.sp, color = t.tx2) },
                action = action,
            )
            val note = outcome ?: loginLine.takeIf { !loggedIn }
            if (note != null) {
                Text(note, fontSize = 13.sp, color = t.tx2, modifier = Modifier.padding(start = 16.dp, end = 16.dp, bottom = 12.dp))
            }
        }
    }
}

/** A label and its value, read by TalkBack as one: "Battery, 3.92 V, about 69 %". */
@Composable
private fun ReadingRow(label: String, value: String) {
    val t = lichen
    val said = "$label, ${value.replace(" · ", ", ")}"
    // The value at the end, or under its label when the line can't hold both.
    TitleAndValue(
        { Text(label, fontSize = 15.sp, color = t.tx2) },
        { Text(value, fontSize = 15.sp, fontWeight = FontWeight.Medium, color = t.tx, style = TextStyle(fontFeatureSettings = "tnum")) },
        Modifier.fillMaxWidth().heightIn(min = 44.dp).padding(horizontal = 16.dp, vertical = 10.dp).clearAndSetSemantics { contentDescription = said },
    )
}

@Composable
private fun ReadingsButton(text: String, onClick: () -> Unit) {
    val t = lichen
    Button(
        onClick,
        colors = ButtonDefaults.buttonColors(containerColor = t.s2, contentColor = t.tx),
        shape = RoundedCornerShape(12.dp),
        contentPadding = PaddingValues(horizontal = 14.dp, vertical = 6.dp),
    ) { Text(text, fontSize = 14.sp, fontWeight = FontWeight.SemiBold) }
}

/** "11.5", "4": [max] decimals at most, none when it has none, in the phone's language. */
private fun decimals(v: Double, max: Int): String {
    val f = java.text.NumberFormat.getNumberInstance()
    f.maximumFractionDigits = max
    f.minimumFractionDigits = 0
    f.isGroupingUsed = false
    return f.format(v)
}

@Composable
private fun readingLabel(kind: ReadingKind): String = stringResource(
    when (kind) {
        ReadingKind.ANALOG_INPUT -> R.string.reading_analog
        ReadingKind.GENERIC -> R.string.reading_generic
        ReadingKind.ILLUMINANCE -> R.string.reading_illuminance
        ReadingKind.TEMPERATURE -> R.string.readings_temperature
        ReadingKind.HUMIDITY -> R.string.reading_humidity
        ReadingKind.PRESSURE -> R.string.reading_pressure
        ReadingKind.VOLTAGE -> R.string.reading_voltage
        ReadingKind.CURRENT -> R.string.reading_current
        ReadingKind.FREQUENCY -> R.string.reading_frequency
        ReadingKind.PERCENTAGE -> R.string.reading_percentage
        ReadingKind.ALTITUDE -> R.string.reading_altitude
        ReadingKind.POWER -> R.string.reading_power
        ReadingKind.DISTANCE -> R.string.reading_distance
        ReadingKind.LATITUDE -> R.string.reading_latitude
        ReadingKind.LONGITUDE -> R.string.reading_longitude
    },
)

/** A value with its unit, as many decimals as it needs: volts two, a position four. A position
 * is only listed here, never put on the map. */
private fun readingValue(v: ReadingValue): String {
    val number = when (v.kind) {
        ReadingKind.VOLTAGE -> "%.2f".format(v.value)
        ReadingKind.LATITUDE, ReadingKind.LONGITUDE -> "%.4f".format(v.value)
        ReadingKind.CURRENT -> decimals(v.value, 3)
        else -> decimals(v.value, 2)
    }
    return when {
        v.unit.isEmpty() -> number
        v.unit == "°" -> "$number°"
        else -> "$number ${v.unit}"
    }
}

/** Its battery over 7 days: a dot per answer joined by a thin line, volts up the side, small. */
@Composable
private fun BatteryChart(points: List<NodeReading>, nowS: Long) {
    val t = lichen
    val measurer = rememberTextMeasurer()
    val volts = points.map { it.batteryMv!!.toDouble() / 1000 }
    val said = stringResource(R.string.readings_chart_said, "%.2f".format(volts.first()), "%.2f".format(volts.last()))
    val lo = minOf(volts.min() - 0.02, volts.max() - 0.1)
    val hi = maxOf(volts.max() + 0.02, lo + 0.1)
    val window = 7 * 86_400L
    val labelStyle = TextStyle(fontSize = 10.sp, color = t.tx2, fontFeatureSettings = "tnum")
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp).clearAndSetSemantics { contentDescription = said }) {
        Canvas(Modifier.fillMaxWidth().height(60.dp * LocalDensity.current.fontScale.coerceIn(1f, 1.5f))) {
            // The side as wide as its widest label (larger text).
            val left = maxOf(34.dp.toPx(), listOf(hi, lo).maxOf { measurer.measure("%.2f".format(it), labelStyle).size.width } + 6.dp.toPx())
            val w = size.width - left
            val h = size.height
            fun y(v: Double) = (h - (v - lo) / (hi - lo) * h).toFloat()
            fun x(at: ULong) = left + ((at.toLong() - (nowS - window)).toFloat() / window) * w
            for (v in listOf(hi, lo)) {
                val yy = y(v).coerceIn(0f, h)
                drawLine(t.line, Offset(left, yy), Offset(size.width, yy), strokeWidth = 1.dp.toPx())
                val text = measurer.measure("%.2f".format(v), labelStyle)
                drawText(text, topLeft = Offset(0f, (yy - text.size.height / 2).coerceIn(0f, h - text.size.height)))
            }
            val pts = points.mapIndexed { i, r -> Offset(x(r.atS).coerceIn(left, size.width), y(volts[i])) }
            val path = Path().apply {
                moveTo(pts.first().x, pts.first().y)
                pts.drop(1).forEach { lineTo(it.x, it.y) }
            }
            drawPath(path, t.tx2.copy(alpha = 0.5f), style = Stroke(1.5.dp.toPx()))
            pts.forEach { drawCircle(t.tx, 3.dp.toPx(), it) }
        }
        Text(stringResource(R.string.range_7d), fontSize = 11.sp, color = t.tx2, modifier = Modifier.padding(top = 4.dp))
    }
}

private const val BAND_ALPHA = 0.6f

/** Logging in to a room server or a repeater (§15.8, §16.5). */
@Composable
fun ServerLoginCard(model: AppModel, key: ByteArray, room: Boolean, focus: FocusRequester? = null) {
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
                    CardRow(stringResource(R.string.log_out), merge = false) {}
                }
            } else {
                CardDivider()
                OutlinedTextField(
                    password, { password = it },
                    Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp).then(if (focus != null) Modifier.focusRequester(focus) else Modifier),
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
                CardRow(stringResource(R.string.console), stringResource(R.string.console_note), merge = false) { Chevron() }
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
