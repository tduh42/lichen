package com.tduhamel.lichen.ui

import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.layout
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.TextStyle
import org.maplibre.android.style.layers.Property
import com.tduhamel.lichen.LichenApplication
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Typeface
import android.view.Gravity
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import org.maplibre.android.camera.CameraPosition
import org.maplibre.android.camera.CameraUpdateFactory
import org.maplibre.android.geometry.LatLng
import org.maplibre.android.geometry.LatLngBounds
import org.maplibre.android.geometry.LatLngQuad
import org.maplibre.android.maps.MapLibreMap
import org.maplibre.android.maps.MapView
import org.maplibre.android.maps.Style
import org.maplibre.android.style.expressions.Expression
import org.maplibre.android.style.layers.FillLayer
import org.maplibre.android.style.layers.LineLayer
import org.maplibre.android.style.layers.PropertyFactory
import org.maplibre.android.style.layers.RasterLayer
import org.maplibre.android.style.layers.SymbolLayer
import org.maplibre.android.style.sources.GeoJsonSource
import org.maplibre.android.style.sources.ImageSource
import org.maplibre.geojson.Feature
import org.maplibre.geojson.FeatureCollection
import org.maplibre.geojson.LineString
import org.maplibre.geojson.Point
import org.maplibre.geojson.Polygon
import kotlin.math.cos
import kotlin.math.max
import kotlin.math.sin
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.ui.Alignment
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.tduhamel.lichen.R
import com.tduhamel.lichen.model.AppModel

/** Map styles from OpenFreeMap: no key, no account (ADR-0011). `AUTO` follows the theme. */
enum class MapStyle(val key: String) {
    AUTO("auto"), DARK("dark"), LIBERTY("liberty"), POSITRON("positron"), BRIGHT("bright");

    fun resolved(dark: Boolean): MapStyle = if (this == AUTO) (if (dark) DARK else LIBERTY) else this

    val url: String get() = "https://tiles.openfreemap.org/styles/$key"
}

/** Something drawn on the map: us, or someone's position with its circle of uncertainty, or a
 * MeshCore node at the position it advertises (protocol.md §16.1). */
data class MapItem(
    val id: String,
    val kind: Kind,
    val lat: Double,
    val lon: Double,
    /** Radius of the circle, metres (none below 25 m). */
    val radius: Double,
    val initials: String,
    val label: String,
    /** ARGB. */
    val color: Int,
    val dark: Boolean,
    /** A node heard directly: a thin ring in its rating's colour (ARGB, ADR-0049). */
    val ring: Int? = null,
    /** Not a stop of the route shown: faded under it ("not on this route"), so that the route
     * reads only its own nodes. Never an SOS. */
    val dimmed: Boolean = false,
) {
    /** `NODE`: a mesh node (ADR-0041 to ADR-0044); `PLACE`: coordinates from a message (ADR-0034). */
    enum class Kind { ME, PEER, SOS, NODE, PLACE }
}

/** A node's route on the map (protocol §16.3): lines between the stops that have a position,
 * each coloured by its leg's rating, dashed where nothing measured it or where stops without a
 * position are skipped; a chip at each line's middle, which TalkBack reads ("hub to crest,
 * −3.0 dB"). [key] is the node's (hex): the camera fits a new one. */
data class MapRoute(val key: String, val lines: List<Line>) {
    data class Line(
        val from: Pair<Double, Double>,
        val to: Pair<Double, Double>,
        /** ARGB. */
        val color: Int,
        val dashed: Boolean,
        val chip: String,
        /** Its chip's colour, ARGB. */
        val chipColor: Int,
        val said: String,
    )
}

/** The route's chips where they fit as the camera moves: each on its line, clear of every marker
 * and its label; a line with no room for its chip has none (reported, the card lists it). */
@Composable
private fun RouteChips(m: MapLibreMap, route: MapRoute, items: List<MapItem>, moved: androidx.compose.runtime.IntState, hidden: ((List<Int>) -> Unit)?) {
    val density = androidx.compose.ui.platform.LocalDensity.current.density
    val measurer = androidx.compose.ui.text.rememberTextMeasurer()
    @Suppress("UNUSED_EXPRESSION") moved.intValue
    fun screen(lat: Double, lon: Double) = m.projection.toScreenLocation(LatLng(lat, lon)).let { Offset(it.x, it.y) }
    val boxes = items.map { markerBox(it, screen(it.lat, it.lon), density) }
    val spots = route.lines.map { line ->
        val text = measurer.measure(line.chip, TextStyle(fontSize = 12.sp, fontWeight = FontWeight.Bold, fontFeatureSettings = "tnum")).size
        val chip = Size(text.width + 14 * density, text.height + 4 * density)
        chipSpot(screen(line.from.first, line.from.second), screen(line.to.first, line.to.second), chip, boxes, 3 * density)
    }
    val none = spots.indices.filter { spots[it] == null }
    LaunchedEffect(none) { hidden?.invoke(none) }
    route.lines.forEachIndexed { i, line ->
        val c = spots[i] ?: return@forEachIndexed
        RouteChip(
            line,
            Modifier.layout { measurable, constraints ->
                val placeable = measurable.measure(constraints.copy(minWidth = 0, minHeight = 0))
                layout(constraints.maxWidth, constraints.maxHeight) {
                    placeable.place(c.x.toInt() - placeable.width / 2, c.y.toInt() - placeable.height / 2)
                }
            },
        )
    }
}

/** Where a leg's chip goes on screen: on the leg from [a] to [b], as near its middle as it can
 * without coming within [margin] of any of [boxes] (the markers and their labels); null when
 * nowhere on the leg is free (a short leg between two markers). */
internal fun chipSpot(a: Offset, b: Offset, chip: Size, boxes: List<Rect>, margin: Float): Offset? {
    for (t in listOf(0.5f, 0.42f, 0.58f, 0.34f, 0.66f, 0.26f, 0.74f, 0.18f, 0.82f)) {
        val c = a + (b - a) * t
        val r = Rect(c.x - chip.width / 2 - margin, c.y - chip.height / 2 - margin, c.x + chip.width / 2 + margin, c.y + chip.height / 2 + margin)
        if (boxes.none { it.overlaps(r) }) return c
    }
    return null
}

/** The screen area a marker takes around its point: its avatar (and rating ring) and the label
 * under it, as [markerBitmap] draws them. */
private fun markerBox(item: MapItem, p: Offset, density: Float): Rect {
    val avatar = (if (item.kind == MapItem.Kind.ME) 22 else 32) * density
    val labelH = if (item.label.isEmpty()) 0f else 16 * density
    val gap = if (item.label.isEmpty()) 0f else 4 * density
    val labelW = if (item.label.isEmpty()) {
        0f
    } else {
        Paint().apply {
            textSize = 11 * density
            typeface = Typeface.create(Typeface.DEFAULT, Typeface.BOLD)
        }.measureText(item.label) + 12 * density
    }
    val w = max(avatar + (if (item.ring != null) 12 else 4) * density, labelW)
    val top = avatar / 2 + (if (item.ring != null) 6 else 3) * density
    return Rect(p.x - w / 2, p.y - top, p.x + w / 2, p.y + avatar / 2 + gap + labelH)
}

/** A leg's chip: its SNR in its rating's colour ("not measured" in `tx2`) on the card colour;
 * TalkBack reads the leg ("hub to crest, −3.0 dB"). */
@Composable
private fun RouteChip(line: MapRoute.Line, modifier: Modifier = Modifier) {
    val t = lichen
    Text(
        line.chip,
        modifier
            .shadow(2.dp, RoundedCornerShape(7.dp))
            .background(t.s1, RoundedCornerShape(7.dp))
            .padding(horizontal = 7.dp, vertical = 2.dp)
            .clearAndSetSemantics { contentDescription = line.said },
        color = Color(line.chipColor),
        fontSize = 12.sp,
        fontWeight = FontWeight.Bold,
        style = TextStyle(fontFeatureSettings = "tnum"),
        maxLines = 1,
    )
}

/** A search estimate on the map: the heat grid as an image over its bounds, and the 68 % and
 * 95 % ellipses as (lat, lon) rings. `version` tells a new estimate. */
class MapOverlay(
    val version: Int,
    val heat: Bitmap?,
    val north: Double,
    val south: Double,
    val west: Double,
    val east: Double,
    val ellipse68: List<Pair<Double, Double>>,
    val ellipse95: List<Pair<Double, Double>>,
) {
    override fun equals(other: Any?) = other is MapOverlay && other.version == version
    override fun hashCode() = version
}

/** MapLibre Native in Compose: markers as a symbol layer, circles as fill and line layers;
 * a search estimate as a raster image and ellipse lines. `margins`: dp kept free when fitting
 * (left, top, right, bottom). */
@Composable
fun LichenMap(
    items: List<MapItem>,
    style: MapStyle,
    recenter: Int,
    modifier: Modifier = Modifier,
    overlay: MapOverlay? = null,
    margins: IntArray = intArrayOf(95, 140, 95, 70),
    /** A place to centre on, once per version (after any `recenter`). */
    focus: com.tduhamel.lichen.model.MapPin? = null,
    /** A node's route, drawn under the markers. */
    route: MapRoute? = null,
    /** A marker tapped: its item's id. */
    onTap: ((String) -> Unit)? = null,
    /** The route's lines (indices) whose chip finds no room on the map, clear of every marker
     * and label: the card lists them. */
    onHiddenChips: ((List<Int>) -> Unit)? = null,
    onCamera: (LatLngBounds, Double) -> Unit,
) {
    // No MapLibre (the JVM UI tests): the markers' labels where the map would be, everything
    // around it as on a phone; and the camera a phone's map reports once it shows them, around
    // them (the area Map layers › Download this area keeps, MP-4).
    if (!LichenApplication.mapsReady) {
        LaunchedEffect(items.size) {
            if (items.isNotEmpty()) {
                val pad = 0.01
                onCamera(
                    LatLngBounds.from(items.maxOf { it.lat } + pad, items.maxOf { it.lon } + pad, items.minOf { it.lat } - pad, items.minOf { it.lon } - pad),
                    12.0,
                )
            }
        }
        val notOnRoute = stringResource(R.string.not_on_route)
        Column(modifier.background(lichen.s1)) {
            for (item in items) {
                if (item.label.isEmpty()) continue
                val tap = onTap?.takeIf { item.kind == MapItem.Kind.NODE }
                Text(
                    item.label, color = lichen.tx2,
                    modifier = (if (tap != null) Modifier.clickable { tap(item.id) } else Modifier)
                        .then(if (item.dimmed) Modifier.alpha(DIMMED).semantics { stateDescription = notOnRoute } else Modifier),
                )
            }
            // The route's chips, as on the map.
            route?.lines?.forEach { RouteChip(it) }
        }
        return
    }
    val context = LocalContext.current
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val mapView = remember { MapView(context).apply { onCreate(null) } }
    // Bumped as the camera moves: the route's chips follow their lines (read by them alone).
    val moved = remember { mutableIntStateOf(0) }
    // The listener is set once: it calls the latest.
    val tapNow by rememberUpdatedState(onTap)
    val casingDark = lichen.dark
    var map by remember { mutableStateOf<MapLibreMap?>(null) }
    var loaded by remember { mutableStateOf<Style?>(null) }
    DisposableEffect(lifecycle) {
        val observer = LifecycleEventObserver { _, e ->
            when (e) {
                Lifecycle.Event.ON_START -> mapView.onStart()
                Lifecycle.Event.ON_RESUME -> mapView.onResume()
                Lifecycle.Event.ON_PAUSE -> mapView.onPause()
                Lifecycle.Event.ON_STOP -> mapView.onStop()
                else -> Unit
            }
        }
        lifecycle.addObserver(observer)
        onDispose {
            lifecycle.removeObserver(observer)
            mapView.onPause()
            mapView.onStop()
            mapView.onDestroy()
        }
    }
    Box(modifier.clipToBounds()) {
        AndroidView({ mapView }, Modifier.fillMaxSize())
        val m = map
        if (m != null && loaded != null && route != null) RouteChips(m, route, items, moved, onHiddenChips)
    }
    LaunchedEffect(Unit) {
        mapView.getMapAsync { m ->
            m.uiSettings.apply {
                compassGravity = Gravity.BOTTOM or Gravity.START
                attributionGravity = Gravity.BOTTOM or Gravity.END
                isLogoEnabled = false
            }
            m.cameraPosition = CameraPosition.Builder().target(LatLng(45.0, 5.5)).zoom(9.0).build()
            m.addOnCameraIdleListener {
                onCamera(m.projection.visibleRegion.latLngBounds, m.cameraPosition.zoom)
                moved.intValue++
            }
            m.addOnCameraMoveListener { moved.intValue++ }
            // A marker tapped: the item it shows; a node first where markers overlap (only a node
            // does something when tapped, its route).
            m.addOnMapClickListener { at ->
                val ids = m.queryRenderedFeatures(m.projection.toScreenLocation(at), ITEMS, ITEMS_DIMMED).mapNotNull { it.getStringProperty("id") }
                val id = ids.firstOrNull { it.startsWith("n:") } ?: ids.firstOrNull()
                if (id != null) tapNow?.invoke(id)
                id != null
            }
            map = m
        }
    }
    LaunchedEffect(map, style) {
        val m = map ?: return@LaunchedEffect
        loaded = null
        m.setStyle(Style.Builder().fromUri(style.url)) { s ->
            s.addSource(GeoJsonSource(CIRCLES))
            s.addSource(GeoJsonSource(ITEMS))
            s.addSource(GeoJsonSource(ELLIPSES))
            s.addSource(GeoJsonSource(ROUTE))
            s.addLayer(
                LineLayer(ELLIPSES, ELLIPSES).withProperties(
                    PropertyFactory.lineColor(android.graphics.Color.rgb(0xDC, 0x26, 0x26)),
                    PropertyFactory.lineWidth(Expression.get("w")),
                ),
            )
            s.addLayer(
                FillLayer("$CIRCLES-fill", CIRCLES).withProperties(
                    PropertyFactory.fillColor(Expression.toColor(Expression.get("color"))),
                    PropertyFactory.fillOpacity(Expression.switchCase(Expression.eq(Expression.get("dim"), Expression.literal(true)), Expression.literal(0.12f * DIMMED), Expression.literal(0.12f))),
                ),
            )
            s.addLayer(
                LineLayer("$CIRCLES-line", CIRCLES).withProperties(
                    PropertyFactory.lineColor(Expression.toColor(Expression.get("color"))),
                    PropertyFactory.lineWidth(1.2f),
                    PropertyFactory.lineOpacity(Expression.switchCase(Expression.eq(Expression.get("dim"), Expression.literal(true)), Expression.literal(DIMMED), Expression.literal(1f))),
                ),
            )
            // The markers a route shown leaves out, faded and under it.
            s.addLayer(
                SymbolLayer(ITEMS_DIMMED, ITEMS).withProperties(
                    PropertyFactory.iconImage(Expression.get("icon")),
                    PropertyFactory.iconAllowOverlap(true),
                    PropertyFactory.iconIgnorePlacement(true),
                    PropertyFactory.iconOpacity(DIMMED),
                    PropertyFactory.symbolSortKey(Expression.get("z")),
                ).withFilter(Expression.eq(Expression.get("dim"), Expression.literal(true))),
            )
            // A route under the markers: a casing, then the measured legs, then the dashed ones.
            s.addLayer(
                LineLayer("$ROUTE-casing", ROUTE).withProperties(
                    PropertyFactory.lineColor(if (casingDark) android.graphics.Color.rgb(0x0E, 0x0E, 0x0D) else android.graphics.Color.WHITE),
                    PropertyFactory.lineWidth(9f),
                    PropertyFactory.lineCap(Property.LINE_CAP_ROUND),
                ),
            )
            s.addLayer(
                LineLayer("$ROUTE-solid", ROUTE).withProperties(
                    PropertyFactory.lineColor(Expression.toColor(Expression.get("color"))),
                    PropertyFactory.lineWidth(5f),
                    PropertyFactory.lineCap(Property.LINE_CAP_ROUND),
                ).withFilter(Expression.eq(Expression.get("dashed"), Expression.literal(false))),
            )
            s.addLayer(
                LineLayer("$ROUTE-dashed", ROUTE).withProperties(
                    PropertyFactory.lineColor(Expression.toColor(Expression.get("color"))),
                    PropertyFactory.lineWidth(5f),
                    PropertyFactory.lineDasharray(arrayOf(1.6f, 1.2f)),
                ).withFilter(Expression.eq(Expression.get("dashed"), Expression.literal(true))),
            )
            s.addLayer(
                SymbolLayer(ITEMS, ITEMS).withProperties(
                    PropertyFactory.iconImage(Expression.get("icon")),
                    PropertyFactory.iconAllowOverlap(true),
                    PropertyFactory.iconIgnorePlacement(true),
                    PropertyFactory.symbolSortKey(Expression.get("z")),
                ).withFilter(Expression.eq(Expression.get("dim"), Expression.literal(false))),
            )
            loaded = s
        }
    }
    LaunchedEffect(loaded, items) {
        val s = loaded ?: return@LaunchedEffect
        val density = context.resources.displayMetrics.density
        val markers = items.map { item ->
            val icon = "marker-${item.hashCode()}"
            if (s.getImage(icon) == null) s.addImage(icon, markerBitmap(item, density))
            Feature.fromGeometry(Point.fromLngLat(item.lon, item.lat)).apply {
                addStringProperty("icon", icon)
                addStringProperty("id", item.id)
                addBooleanProperty("dim", item.dimmed)
                addNumberProperty("z", when (item.kind) { MapItem.Kind.SOS -> 3; MapItem.Kind.PLACE -> 2; MapItem.Kind.ME -> 1; MapItem.Kind.PEER -> 0; MapItem.Kind.NODE -> -1 })
            }
        }
        val circles = items.filter { it.radius >= 25 }.map { item ->
            val ring = (0..64).map { i ->
                val a = i / 64.0 * 2 * Math.PI
                val dlat = item.radius * cos(a) / 111_320
                val dlon = item.radius * sin(a) / (111_320 * max(0.01, cos(Math.toRadians(item.lat))))
                Point.fromLngLat(item.lon + dlon, (item.lat + dlat).coerceIn(-90.0, 90.0))
            }
            Feature.fromGeometry(Polygon.fromLngLats(listOf(ring))).apply {
                addStringProperty("color", "#%06X".format(item.color and 0xFFFFFF))
                addBooleanProperty("dim", item.dimmed)
            }
        }
        s.getSourceAs<GeoJsonSource>(ITEMS)?.setGeoJson(FeatureCollection.fromFeatures(markers))
        s.getSourceAs<GeoJsonSource>(CIRCLES)?.setGeoJson(FeatureCollection.fromFeatures(circles))
    }
    LaunchedEffect(loaded, route) {
        val s = loaded ?: return@LaunchedEffect
        val lines = route?.lines.orEmpty().map { l ->
            Feature.fromGeometry(LineString.fromLngLats(listOf(Point.fromLngLat(l.from.second, l.from.first), Point.fromLngLat(l.to.second, l.to.first)))).apply {
                addStringProperty("color", "#%06X".format(l.color and 0xFFFFFF))
                addBooleanProperty("dashed", l.dashed)
            }
        }
        s.getSourceAs<GeoJsonSource>(ROUTE)?.setGeoJson(FeatureCollection.fromFeatures(lines))
    }
    LaunchedEffect(loaded, overlay) {
        val s = loaded ?: return@LaunchedEffect
        s.getLayer(HEAT)?.let { s.removeLayer(it) }
        s.getSource(HEAT)?.let { s.removeSource(it) }
        val o = overlay
        val heat = o?.heat
        if (o != null && heat != null) {
            val quad = LatLngQuad(
                LatLng(o.north, o.west), LatLng(o.north, o.east), LatLng(o.south, o.east), LatLng(o.south, o.west),
            )
            s.addSource(ImageSource(HEAT, quad, heat))
            s.addLayerBelow(
                RasterLayer(HEAT, HEAT).withProperties(PropertyFactory.rasterOpacity(0.8f), PropertyFactory.rasterFadeDuration(0f)),
                ELLIPSES,
            )
        }
        val lines = if (o == null) emptyList() else listOf(o.ellipse95 to 1.5f, o.ellipse68 to 2.5f).filter { it.first.isNotEmpty() }.map { (ring, w) ->
            Feature.fromGeometry(LineString.fromLngLats(ring.map { Point.fromLngLat(it.second, it.first) })).apply {
                addNumberProperty("w", w)
            }
        }
        s.getSourceAs<GeoJsonSource>(ELLIPSES)?.setGeoJson(FeatureCollection.fromFeatures(lines))
    }
    LaunchedEffect(recenter, loaded) {
        val m = map ?: return@LaunchedEffect
        if (loaded == null || items.isEmpty()) return@LaunchedEffect
        fit(m, items, overlay, margins, context.resources.displayMetrics.density)
    }
    LaunchedEffect(focus?.version, loaded) {
        val m = map ?: return@LaunchedEffect
        val f = focus ?: return@LaunchedEffect
        if (loaded == null) return@LaunchedEffect
        m.animateCamera(CameraUpdateFactory.newLatLngZoom(LatLng(f.lat, f.lon), max(m.cameraPosition.zoom, 14.0)), 600)
    }
    // A new route: the camera takes all of it in, the card below kept clear. After the fits
    // above: on the map's first load, the route wins.
    LaunchedEffect(route?.key, loaded) {
        val m = map ?: return@LaunchedEffect
        val r = route ?: return@LaunchedEffect
        if (loaded == null || r.lines.isEmpty()) return@LaunchedEffect
        val b = LatLngBounds.Builder()
        r.lines.forEach {
            b.include(LatLng(it.from.first.coerceIn(-LAT_MAX, LAT_MAX), it.from.second))
            b.include(LatLng(it.to.first.coerceIn(-LAT_MAX, LAT_MAX), it.to.second))
        }
        val d = context.resources.displayMetrics.density
        val camera = runCatching { m.getCameraForLatLngBounds(b.build(), intArrayOf((60 * d).toInt(), (150 * d).toInt(), (60 * d).toInt(), (230 * d).toInt())) }.getOrNull()
            ?: return@LaunchedEffect
        m.animateCamera(CameraUpdateFactory.newCameraPosition(CameraPosition.Builder(camera).zoom(minOf(camera.zoom, 15.0)).build()), 600)
    }
}

/** Every marker with its whole circle of uncertainty, never closer than street level. */
private fun fit(m: MapLibreMap, all: List<MapItem>, overlay: MapOverlay?, margins: IntArray, density: Float) {
    // People first: nodes count only when nobody else is on the map.
    val items = all.filter { it.kind != MapItem.Kind.NODE }.ifEmpty { all }
    val first = items.first()
    if (items.size == 1 && first.radius < 500 && overlay == null) {
        m.animateCamera(CameraUpdateFactory.newLatLngZoom(LatLng(first.lat, first.lon), max(m.cameraPosition.zoom, 13.0)), 600)
        return
    }
    val b = LatLngBounds.Builder()
    for (i in items) {
        val dlat = i.radius / 111_320
        val dlon = i.radius / (111_320 * max(0.01, cos(Math.toRadians(i.lat))))
        // Positions come from other people: near a pole, a circle reaches past ±90°, which
        // LatLng refuses with an exception, and past what the map projects (review of
        // 2026-11-15, SR-37).
        b.include(LatLng((i.lat - dlat).coerceIn(-LAT_MAX, LAT_MAX), i.lon - dlon))
        b.include(LatLng((i.lat + dlat).coerceIn(-LAT_MAX, LAT_MAX), i.lon + dlon))
    }
    // And the search estimate's 95 % ellipse.
    overlay?.ellipse95?.forEach { b.include(LatLng(it.first.coerceIn(-LAT_MAX, LAT_MAX), it.second)) }
    val pad = IntArray(4) { (margins[it] * density).toInt() }
    val camera = runCatching { m.getCameraForLatLngBounds(b.build(), pad) }.getOrNull() ?: return
    val capped = CameraPosition.Builder(camera).zoom(minOf(camera.zoom, 15.0)).build()
    m.animateCamera(CameraUpdateFactory.newCameraPosition(capped), 600)
}

/** An avatar (or our blue dot) with its label below; the avatar's centre is the bitmap's. */
private fun markerBitmap(item: MapItem, density: Float): Bitmap {
    val avatar = (if (item.kind == MapItem.Kind.ME) 22 else 32) * density
    val labelPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        textSize = 11 * density
        typeface = Typeface.create(Typeface.DEFAULT, Typeface.BOLD)
        color = when {
            item.kind == MapItem.Kind.SOS -> android.graphics.Color.WHITE
            item.dark -> 0xFFF4F4F0.toInt()
            else -> 0xFF141413.toInt()
        }
    }
    val padX = 6 * density
    val labelH = if (item.label.isEmpty()) 0f else 16 * density
    val gap = if (item.label.isEmpty()) 0f else 4 * density
    val labelW = if (item.label.isEmpty()) 0f else labelPaint.measureText(item.label) + 2 * padX
    // A node's rating ring takes a little more room around it.
    val w = max(avatar + (if (item.ring != null) 12 else 4) * density, labelW)
    val h = avatar + 2 * (gap + labelH) + 4 * density
    val bmp = Bitmap.createBitmap(w.toInt().coerceAtLeast(1), h.toInt().coerceAtLeast(1), Bitmap.Config.ARGB_8888)
    val c = Canvas(bmp)
    val cx = bmp.width / 2f
    val cy = bmp.height / 2f
    val ring = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = if (item.dark) 0xFF0E0E0D.toInt() else android.graphics.Color.WHITE }
    val fill = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = item.color }
    val r = avatar / 2
    if (item.kind == MapItem.Kind.NODE) {
        // A node: a rounded square; heard directly, a thin ring in its rating's colour.
        item.ring?.let { color ->
            val o2 = r + 4.5f * density
            c.drawRoundRect(
                RectF(cx - o2, cy - o2, cx + o2, cy + o2), 11 * density, 11 * density,
                Paint(Paint.ANTI_ALIAS_FLAG).apply {
                    this.color = color
                    style = Paint.Style.STROKE
                    strokeWidth = 2 * density
                },
            )
        }
        val o = r + 2 * density
        c.drawRoundRect(RectF(cx - o, cy - o, cx + o, cy + o), 9 * density, 9 * density, ring)
        c.drawRoundRect(RectF(cx - r, cy - r, cx + r, cy + r), 8 * density, 8 * density, fill)
    } else {
        c.drawCircle(cx, cy, r + (if (item.kind == MapItem.Kind.ME) 3 else 2) * density, ring)
        c.drawCircle(cx, cy, r, fill)
    }
    if (item.kind == MapItem.Kind.PLACE) {
        // A pin: a white dot in the disc.
        c.drawCircle(cx, cy, r * 0.32f, Paint(Paint.ANTI_ALIAS_FLAG).apply { color = android.graphics.Color.WHITE })
    } else if (item.kind != MapItem.Kind.ME) {
        val initials = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            textSize = 12 * density
            typeface = Typeface.create(Typeface.DEFAULT, Typeface.BOLD)
            color = 0xFF141413.toInt()
            textAlign = Paint.Align.CENTER
        }
        c.drawText(item.initials, cx, cy - (initials.descent() + initials.ascent()) / 2, initials)
    }
    if (item.label.isNotEmpty()) {
        val top = cy + r + gap
        val bg = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = when {
                item.kind == MapItem.Kind.SOS -> 0xFFDC2626.toInt()
                item.dark -> 0xFF1A1A18.toInt()
                else -> android.graphics.Color.WHITE
            }
        }
        val rect = RectF(cx - labelW / 2, top, cx + labelW / 2, top + labelH)
        c.drawRoundRect(rect, 6 * density, 6 * density, bg)
        c.drawText(item.label, rect.left + padX, rect.centerY() - (labelPaint.descent() + labelPaint.ascent()) / 2, labelPaint)
    }
    return bmp
}

private const val ITEMS = "lichen-items"
private const val ROUTE = "lichen-route"
private const val ITEMS_DIMMED = "lichen-items-dimmed"

/** A marker that is not a stop of the route shown: about a third of its colour (as on iOS). */
private const val DIMMED = 0.35f
private const val CIRCLES = "lichen-circles"
private const val ELLIPSES = "lichen-search-ellipses"
private const val HEAT = "lichen-search-heat"

/** Web Mercator ends here: a camera asked to show beyond it has no answer. */
private const val LAT_MAX = 85.0

/**
 * With Tor on, map tiles would go to OpenFreeMap directly, outside Tor, and show the area being
 * looked at: the map waits until allowed, for this run of the app (review of 2026-11-15, SR-36).
 * Positions themselves never leave the phone this way.
 */
@Composable
fun MapGate(model: AppModel, content: @Composable () -> Unit) {
    val t = lichen
    if (model.torEnabled && !model.mapWithTorAllowed) {
        Column(
            Modifier.fillMaxSize().background(t.bg).padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically),
        ) {
            Text(stringResource(R.string.map_waits_tor), fontSize = 17.sp, fontWeight = FontWeight.SemiBold, color = t.tx)
            Text(stringResource(R.string.map_waits_tor_note), fontSize = 13.sp, color = t.tx2, textAlign = TextAlign.Center)
            SecondaryButton(stringResource(R.string.map_load_once)) { model.mapWithTorAllowed = true }
        }
    } else {
        content()
    }
}

