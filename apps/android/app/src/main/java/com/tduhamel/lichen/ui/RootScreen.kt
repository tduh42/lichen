package com.tduhamel.lichen.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.Chat
import androidx.compose.material.icons.outlined.GpsFixed
import androidx.compose.material.icons.outlined.Map
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material3.Badge as M3Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import com.tduhamel.lichen.R
import com.tduhamel.lichen.model.AppModel
import com.tduhamel.lichen.model.AppModel.Tab
import com.tduhamel.lichen.model.ChatId
import com.tduhamel.lichen.model.activeIncidents
import com.tduhamel.lichen.model.unhex

private val TabRoutes = mapOf(
    Tab.MESSAGES to "messages",
    Tab.MAP to "map",
    Tab.SEARCH to "search",
    Tab.SETTINGS to "settings",
)

/** The four tabs, and the chats above them. */
@Composable
fun RootScreen(model: AppModel) {
    model.identityProblem?.let {
        IdentityProblemScreen(model, it)
        return
    }
    if (!model.onboarded) {
        Onboarding(model)
        return
    }
    val nav = rememberNavController()
    val entry by nav.currentBackStackEntryAsState()
    val route = entry?.destination?.route
    val onTab = route in TabRoutes.values

    // A chat asked for (debug extra `open`, later a notification).
    LaunchedEffect(model.openChat) {
        val chat = model.openChat ?: return@LaunchedEffect
        model.openChat = null
        nav.navigate("chat/${chat.route}") { launchSingleTop = true }
    }

    // Each screen handles its own system bars; the tab bar its own.
    // Debug extra `screen`: a screen to show at launch (screenshots).
    LaunchedEffect(Unit) {
        when (model.debugScreen) {
            "contact", "identity", "sos" -> nav.navigate(model.debugScreen!!)
        }
    }
    // Debug extra `screen join`: the scanner opens on the demo's invitation.
    LaunchedEffect(model.debugInvite) { if (model.debugInvite != null) nav.navigate("contact") }

    Scaffold(
        containerColor = lichen.bg,
        contentWindowInsets = WindowInsets(0),
        bottomBar = { if (onTab) TabBar(model, nav, route) },
    ) { padding ->
        // Someone sent an SOS: a full-screen alert, wherever we are, once the app lock lets it
        // show (the lock's cover says an SOS came in meanwhile).
        fun tab(r: String) = nav.navigate(r) {
            popUpTo(nav.graph.startDestinationId) { saveState = true }
            launchSingleTop = true
        }
        model.sosAlert?.takeIf { !model.lock.locked }?.let { from ->
            SosReceivedDialog(
                model, from,
                search = { tab("search") },
                reply = { model.openChat = ChatId.contact(from.unhex()) },
                map = { tab("map") },
            )
        }
        // An SOS notification, or the debug extra `screen search`: the Search tab on them.
        LaunchedEffect(model.openSearch) {
            val who = model.openSearch ?: return@LaunchedEffect
            model.openSearch = null
            model.searchTarget = who
            tab("search")
        }
        NavHost(
            nav,
            startDestination = TabRoutes.getValue(model.tab),
            modifier = Modifier.padding(bottom = padding.calculateBottomPadding()),
        ) {
            composable("messages") {
                ConversationsScreen(
                    model,
                    open = { nav.navigate("chat/${it.route}") },
                    pair = { nav.navigate("pairing") },
                    addContact = { nav.navigate("contact") },
                    search = { nav.navigate("search-messages") },
                )
            }
            composable("search-messages") {
                SearchMessagesScreen(model, back = { nav.popBackStack() }) { chat, id ->
                    if (id != null) model.focus = chat to id
                    nav.navigate("chat/${chat.route}") { popUpTo("messages") }
                }
            }
            composable("map") { MapScreen(model, sos = { nav.navigate("sos") }) }
            composable("search") { SearchScreen(model) }
            composable("sos") { SosScreen(model, close = { nav.popBackStack() }) }
            composable("settings") {
                SettingsScreen(model, pair = { nav.navigate("pairing") }, identity = { nav.navigate("identity") })
            }
            composable("identity") { IdentityScreen(model, back = { nav.popBackStack() }) }
            composable("contact") { ContactScreen(model, back = { nav.popBackStack() }) }
            composable("group/{id}") { e ->
                val chat = e.arguments?.getString("id")?.let(ChatId::fromRoute)
                if (chat != null) {
                    GroupScreen(model, chat, back = { nav.popBackStack() }, left = { nav.popBackStack("messages", inclusive = false) })
                }
            }
            composable("pairing") { PairingScreen(model, back = { nav.popBackStack() }) }
            composable("chat/{id}") { e ->
                val chat = e.arguments?.getString("id")?.let(ChatId::fromRoute)
                if (chat != null) {
                    ChatScreen(model, chat, back = { nav.popBackStack() }, details = { nav.navigate("group/${chat.route}") })
                }
            }
        }
    }
}

@Composable
private fun TabBar(model: AppModel, nav: NavHostController, route: String?) {
    val t = lichen
    NavigationBar(containerColor = t.s1) {
        val items = listOf(
            Triple(Tab.MESSAGES, Icons.AutoMirrored.Outlined.Chat, R.string.tab_messages),
            Triple(Tab.MAP, Icons.Outlined.Map, R.string.tab_map),
            Triple(Tab.SEARCH, Icons.Outlined.GpsFixed, R.string.tab_search),
            Triple(Tab.SETTINGS, Icons.Outlined.Settings, R.string.tab_settings),
        )
        for ((tab, icon, label) in items) {
            val r = TabRoutes.getValue(tab)
            NavigationBarItem(
                selected = route == r,
                onClick = {
                    model.tab = tab
                    nav.navigate(r) {
                        popUpTo(nav.graph.startDestinationId) { saveState = true }
                        launchSingleTop = true
                        restoreState = true
                    }
                },
                icon = {
                    TabIcon(
                        icon,
                        when (tab) {
                            Tab.MESSAGES -> model.totalUnread
                            // A search to follow.
                            Tab.SEARCH -> model.activeIncidents.size
                            else -> 0
                        },
                    )
                },
                label = { Text(stringResource(label), fontSize = 12.sp, fontWeight = FontWeight.Medium) },
                colors = NavigationBarItemDefaults.colors(
                    selectedIconColor = t.tx,
                    selectedTextColor = t.tx,
                    unselectedIconColor = t.tx2,
                    unselectedTextColor = t.tx2,
                    indicatorColor = t.s2,
                ),
            )
        }
    }
}

@Composable
private fun TabIcon(icon: ImageVector, badge: Int) {
    if (badge > 0) {
        BadgedBox(badge = { M3Badge(containerColor = lichen.actBg, contentColor = lichen.actFg) { Text("$badge") } }) {
            Icon(icon, null)
        }
    } else {
        Icon(icon, null)
    }
}
