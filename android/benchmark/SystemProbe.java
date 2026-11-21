package local.hex.measure;

import android.app.Activity;
import android.content.Intent;
import android.os.*;
import android.media.*;
import android.text.Editable;
import android.text.TextWatcher;
import android.view.Gravity;
import android.widget.LinearLayout;
import android.widget.TextView;
import android.speech.*;
import android.util.Log;
import android.widget.EditText;
import java.util.ArrayList;

/** Disposable foreground probe, never packaged in Hex. Only the synthetic corpus is logged. */
public class SystemProbe extends Activity {
    private SpeechRecognizer speech;
    private String active;
    private long stop, ready;
    private AudioTrack music;
    private AudioFocusRequest focus;
    public void onCreate(Bundle b) { super.onCreate(b); setContentView(new EditText(this)); handle(getIntent()); }
    protected void onNewIntent(Intent i) { super.onNewIntent(i); handle(i); }
    protected void onDestroy() {
        if (speech != null) speech.destroy();
        if (music != null) { music.stop(); music.release(); }
        if (focus != null) getSystemService(AudioManager.class).abandonAudioFocusRequest(focus);
        super.onDestroy();
    }
    private void handle(Intent i) {
        String id=i.getStringExtra("id"), op=i.getStringExtra("op"); if (op==null) return;
        if ("editor".equals(op)) { if (music == null) openEditor(); return; }
        if ("stop".equals(op)) { if(id.equals(active)){ stop=now(); speech.stopListening(); } return; }
        if(!SpeechRecognizer.isOnDeviceRecognitionAvailable(this)){log(id,"unavailable");return;}
        if(speech==null) speech=SpeechRecognizer.createOnDeviceSpeechRecognizer(this);
        Intent request=new Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH)
            .putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL,RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
            .putExtra(RecognizerIntent.EXTRA_LANGUAGE,i.getStringExtra("lang"))
            .putExtra(RecognizerIntent.EXTRA_PREFER_OFFLINE,true);
        if ("download".equals(op)){
            String locale=i.getStringExtra("lang");
            speech.triggerModelDownload(request,getMainExecutor(),new ModelDownloadListener(){
                public void onProgress(int pct){} public void onSuccess(){log("pack",locale+" installed");}
                public void onScheduled(){log("pack",locale+" scheduled");} public void onError(int e){log("pack",locale+" error="+e);}
            });
            return;
        }
        if ("support".equals(op)){
            speech.checkRecognitionSupport(request,getMainExecutor(),new RecognitionSupportCallback(){
                public void onSupportResult(RecognitionSupport s){log(id,"installed="+s.getInstalledOnDeviceLanguages()+" supported="+s.getSupportedOnDeviceLanguages()+" pending="+s.getPendingOnDeviceLanguages());}
                public void onError(int code){log(id,"supportError="+code);}
            });return;
        }
        active=id;stop=0;ready=0;
        speech.setRecognitionListener(new RecognitionListener(){
            public void onReadyForSpeech(Bundle b){ready=now();log(id,"ready "+ready);}
            public void onBeginningOfSpeech(){} public void onRmsChanged(float f){} public void onBufferReceived(byte[] b){}
            public void onEndOfSpeech(){} public void onPartialResults(Bundle b){} public void onEvent(int x,Bundle b){}
            public void onError(int err){active=null;log(id,"error "+err);}
            public void onResults(Bundle b){active=null;ArrayList<String> v=b.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION);
                log(id,"result "+now()+" "+ready+" "+stop+" "+(v==null||v.isEmpty()?"":v.get(0))); }
        });
        speech.startListening(request);
    }
    // An unrelated editor/UID with an actual playing media track. A focus interruption pauses it.
    private void openEditor() {
        LinearLayout page = new LinearLayout(this);
        page.setOrientation(LinearLayout.VERTICAL);
        page.setPadding(48, 48, 48, 48);
        TextView title = new TextView(this);
        title.setText("External editor"); title.setTextSize(24);
        page.addView(title);
        EditText field = new EditText(this);
        field.setGravity(Gravity.TOP); field.setMinLines(3);
        page.addView(field); setContentView(page);
        AudioAttributes attrs = new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA)
                .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC).build();
        int rate = 16000;
        short[] tune = new short[rate * 2];
        double[] notes = {220, 261.63, 329.63, 392};
        for (int n=0; n<tune.length; n++)
            tune[n] = (short)(4500*Math.sin(2*Math.PI*notes[n/(rate/2)]*n/rate)*Math.exp(-5.0*(n%(rate/2))/rate));
        music = new AudioTrack.Builder().setAudioAttributes(attrs).setAudioFormat(new AudioFormat.Builder()
                .setSampleRate(rate).setChannelMask(AudioFormat.CHANNEL_OUT_MONO).setEncoding(AudioFormat.ENCODING_PCM_16BIT).build())
                .setTransferMode(AudioTrack.MODE_STATIC).setBufferSizeInBytes(tune.length*2).build();
        music.write(tune, 0, tune.length); music.setLoopPoints(0, tune.length, -1);
        focus = new AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN).setAudioAttributes(attrs)
                .setOnAudioFocusChangeListener(change -> {
                    log("media", "focus " + change);
                    if (change < 0) music.pause();
                }).build();
        if (getSystemService(AudioManager.class).requestAudioFocus(focus) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED) music.play();
        log("media","start "+now()+" state="+music.getPlayState());
        field.addTextChangedListener(new TextWatcher() {
            public void beforeTextChanged(CharSequence t, int st, int count, int after) {}
            public void onTextChanged(CharSequence t, int st, int before, int count) {
                if (count > 0) log("editor", "insert "+now()+" state="+music.getPlayState()+" frames="+music.getPlaybackHeadPosition());
            }
            public void afterTextChanged(Editable e) {}
        });
    }
    private static long now(){return SystemClock.elapsedRealtime();}
    private static void log(String id,String s){Log.i("HexMeasure",id+"|"+s);}
}
