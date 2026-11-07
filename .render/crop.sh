set -e
cd captures/flow
ffmpeg -loglevel error -y -i flow-silent.mp4 -i flow.wav -vf crop=1960:1520:402:0 -c:v libx264 -preset slow -crf 22 -pix_fmt yuv420p -movflags +faststart -c:a aac -b:a 160k -shortest flow-dark-cropped.mp4
ls -la flow-dark-cropped.mp4
