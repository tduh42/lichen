# UniFFI's bindings reach the Rust library through JNA, by reflection.
-dontwarn java.awt.**
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class com.tduhamel.lichen.core.** { *; }
