import { Camera, CameraView } from "expo-camera";
import { Stack } from "expo-router";
import { useState } from "react";
import {
  AppState,
  Linking,
  Platform,
  SafeAreaView,
  StatusBar,
  StyleSheet,
} from "react-native";

export default function Home() {
    const [scanned, setScanned] = useState(false);
    return (
    <SafeAreaView style={StyleSheet.absoluteFillObject}>
        <Stack.Screen
        options={{
          title: "Overview",
          headerShown: false,
        }}
        />
        {Platform.OS === "android" ? <StatusBar hidden /> : null}
        <CameraView
            style={StyleSheet.absoluteFillObject}
            facing="back"
            onBarcodeScanned={({ data }) => {
                if (!scanned) {
                    setScanned(true);
                    Linking.openURL(data);
                }
            }}
        />
    </SafeAreaView>
  );
}