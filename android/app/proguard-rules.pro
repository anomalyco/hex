# libhexspeech resolves this class and method by their JNI names.
-keep class ly.anoma.hex.speech.Whisper {
    native <methods>;
}

# libhex_keyboard (crates/keyboard/ffi/src/jni.rs) resolves KeyboardCore's natives by their JNI
# names and makes SwipeReading through this constructor.
-keep class ly.anoma.hex.typing.KeyboardCore {
    native <methods>;
}
-keep class ly.anoma.hex.typing.SwipeReading {
    <init>(java.lang.String[], double[], int);
}

# Drop diagnostic calls from managed release code, including dependencies.
-assumenosideeffects class android.util.Log {
    public static boolean isLoggable(java.lang.String, int) return false;
    public static int v(...);
    public static int d(...);
    public static int i(...);
    public static int w(...);
    public static int e(...);
    public static int wtf(...);
    public static int println(...);
}
