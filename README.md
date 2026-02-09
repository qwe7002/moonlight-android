# Deragabu

[Moonlight for Android](https://moonlight-stream.org) is an open source client for [Sunshine](https://github.com/LizardByte/Sunshine).

Deragabu will allow you to stream your full collection of games from your PC to your Android device,
whether in your own home or over the internet.

Moonlight also has a [PC client](https://github.com/moonlight-stream/moonlight-qt) and [iOS/tvOS client](https://github.com/moonlight-stream/moonlight-ios).

You can follow development on our [Discord server](https://moonlight-stream.org/discord) and help translate Moonlight into your language on [Weblate](https://hosted.weblate.org/projects/moonlight/moonlight-android/).

## Features
* Stream games from your PC to your Android device
* Support Sunshine
* On-screen keyboard input with text entry bar
  * **Send** - Send text without Enter key
  * **Send + Enter** - Send text and automatically press Enter
  * **Cancel** - Close the input bar

## Building
* Install Android Studio and the Android NDK
* Run ‘git submodule update --init --recursive’ from within moonlight-android/
* In moonlight-android/, create a file called ‘local.properties’. Add an ‘ndk.dir=’ property to the local.properties file and set it equal to your NDK directory.
* Build the APK using Android Studio or gradle

## Authors

* [Cameron Gutman](https://github.com/cgutman)  
* [Diego Waxemberg](https://github.com/dwaxemberg)  
* [Aaron Neyer](https://github.com/Aaronneyer)  
* [Andrew Hennessy](https://github.com/yetanothername)
* [qwe7002](https://github.com/qwe7002)

Moonlight is the work of students at [Case Western](http://case.edu) and was
started as a project at [MHacks](http://mhacks.org).
