package com.tduhamel.lichen.ui

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

/** Map styles from OpenFreeMap: no key, no account (ADR-0011). `AUTO` follows the theme. */
enum class MapStyle(val key: String) {
    AUTO("auto"), DARK("dark"), LIBERTY("liberty"), POSITRON("positron"), BRIGHT("bright");

    fun resolved(dark: Boolean): MapStyle = if (this == AUTO) (if (dark) DARK else LIBERTY) else this

    val url: String get() = "https://tiles.openfreemap.org/styles/$key"
}

/** Something drawn on the map: us, or someone's position with its circle of uncertainty. */
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
) {
    enum class Kind { ME, PEER, SOS }
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
    onCamera: (LatLngBounds, Double) -> Unit,
) {
    val context = LocalContext.current
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val mapView = remember { MapView(context).apply { onCreate(null) } }
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
    AndroidView({ mapView }, modifier)
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
            s.addLayer(
                LineLayer(ELLIPSES, ELLIPSES).withProperties(
                    PropertyFactory.lineColor(android.graphics.Color.rgb(0xDC, 0x26, 0x26)),
                    PropertyFactory.lineWidth(Expression.get("w")),
                ),
            )
            s.addLayer(
                FillLayer("$CIRCLES-fill", CIRCLES).withProperties(
                    PropertyFactory.fillColor(Expression.toColor(Expression.get("color"))),
                    PropertyFactory.fillOpacity(0.12f),
                ),
            )
            s.addLayer(
                LineLayer("$CIRCLES-line", CIRCLES).withProperties(
                    PropertyFactory.lineColor(Expression.toColor(Expression.get("color"))),
                    PropertyFactory.lineWidth(1.2f),
                ),
            )
            s.addLayer(
                SymbolLayer(ITEMS, ITEMS).withProperties(
                    PropertyFactory.iconImage(Expression.get("icon")),
                    PropertyFactory.iconAllowOverlap(true),
                    PropertyFactory.iconIgnorePlacement(true),
                    PropertyFactory.symbolSortKey(Expression.get("z")),
                ),
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
                addNumberProperty("z", when (item.kind) { MapItem.Kind.SOS -> 2; MapItem.Kind.ME -> 1; MapItem.Kind.PEER -> 0 })
            }
        }
        val circles = items.filter { it.radius >= 25 }.map { item ->
            val ring = (0..64).map { i ->
                val a = i / 64.0 * 2 * Math.PI
                val dlat = item.radius * cos(a) / 111_320
                val dlon = item.radius * sin(a) / (111_320 * cos(Math.toRadians(item.lat)))
                Point.fromLngLat(item.lon + dlon, item.lat + dlat)
            }
            Feature.fromGeometry(Polygon.fromLngLats(listOf(ring))).apply {
                addStringProperty("color", "#%06X".format(item.color and 0xFFFFFF))
            }
        }
        s.getSourceAs<GeoJsonSource>(ITEMS)?.setGeoJson(FeatureCollection.fromFeatures(markers))
        s.getSourceAs<GeoJsonSource>(CIRCLES)?.setGeoJson(FeatureCollection.fromFeatures(circles))
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
}

/** Every marker with its whole circle of uncertainty, never closer than street level. */
private fun fit(m: MapLibreMap, items: List<MapItem>, overlay: MapOverlay?, margins: IntArray, density: Float) {
    val first = items.first()
    if (items.size == 1 && first.radius < 500 && overlay == null) {
        m.animateCamera(CameraUpdateFactory.newLatLngZoom(LatLng(first.lat, first.lon), max(m.cameraPosition.zoom, 13.0)), 600)
        return
    }
    val b = LatLngBounds.Builder()
    for (i in items) {
        val dlat = i.radius / 111_320
        val dlon = i.radius / (111_320 * max(0.01, cos(Math.toRadians(i.lat))))
        b.include(LatLng(i.lat - dlat, i.lon - dlon))
        b.include(LatLng(i.lat + dlat, i.lon + dlon))
    }
    // And the search estimate's 95 % ellipse.
    overlay?.ellipse95?.forEach { b.include(LatLng(it.first, it.second)) }
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
    val w = max(avatar + 4 * density, labelW)
    val h = avatar + 2 * (gap + labelH) + 4 * density
    val bmp = Bitmap.createBitmap(w.toInt().coerceAtLeast(1), h.toInt().coerceAtLeast(1), Bitmap.Config.ARGB_8888)
    val c = Canvas(bmp)
    val cx = bmp.width / 2f
    val cy = bmp.height / 2f
    val ring = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = if (item.dark) 0xFF0E0E0D.toInt() else android.graphics.Color.WHITE }
    val fill = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = item.color }
    val r = avatar / 2
    c.drawCircle(cx, cy, r + (if (item.kind == MapItem.Kind.ME) 3 else 2) * density, ring)
    c.drawCircle(cx, cy, r, fill)
    if (item.kind != MapItem.Kind.ME) {
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
private const val CIRCLES = "lichen-circles"
private const val ELLIPSES = "lichen-search-ellipses"
private const val HEAT = "lichen-search-heat"
