"""Feed PCM16 to the authenticated guest microphone; back-pressure preserves every packet."""
import os, time, wave
from pathlib import Path
import grpc, emulator_controller_pb2 as pb, emulator_controller_pb2_grpc as service

def inject(filename):
    port=os.environ.get("HEX_GRPC_PORT","8566")
    for f in (Path.home()/"Library/Caches/TemporaryItems/avd/running").glob("pid_*.ini"):
        entries=dict(line.rstrip().split("=",1) for line in f.read_text().splitlines() if "=" in line)
        if entries.get("grpc.port")==port:break
    else:raise RuntimeError("Missing emulator discovery for "+port)
    with grpc.insecure_channel("127.0.0.1:"+port) as channel, wave.open(str(filename),"rb") as wav:
        rate=wav.getframerate()
        assert (wav.getnchannels(),wav.getsampwidth(),rate)==(1,2,16000)
        fmt=pb.AudioFormat(samplingRate=rate,channels=pb.AudioFormat.Mono,format=pb.AudioFormat.AUD_FMT_S16,mode=pb.AudioFormat.MODE_UNSPECIFIED)
        def packets():
            start=time.monotonic()
            while (data:=wav.readframes(rate//50)):
                yield pb.AudioPacket(format=fmt,audio=data)
                time.sleep(max(0,start+wav.tell()/rate-time.monotonic()))
        service.EmulatorControllerStub(channel).injectAudio(packets(),metadata=(("authorization","Bearer "+entries["grpc.token"]),),timeout=wav.getnframes()/rate+15)
