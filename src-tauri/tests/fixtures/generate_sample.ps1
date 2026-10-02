# Regenerates sample.wav using Windows' built-in SAPI TTS — a real spoken
# 16kHz mono clip used by src/stt.rs's `transcribes_real_speech` test.
Add-Type -AssemblyName System.Speech
$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
$fmt = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(
    16000, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono
)
$path = Join-Path $PSScriptRoot "sample.wav"
$synth.SetOutputToWaveFile($path, $fmt)
$synth.Rate = -2
$synth.Speak("The quick brown fox jumps over the lazy dog.")
$synth.Dispose()
Write-Output "wrote $path"
