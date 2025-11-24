import { CameraView } from "expo-camera";
import { Stack } from "expo-router";
import { useState } from "react";
import {
    Platform,
    SafeAreaView,
    StatusBar,
    StyleSheet,
    Alert,
} from "react-native";

export default function Home() {
    const [scanned, setScanned] = useState(false);
    const SERVER_URL = "http://192.168.0.3:3000/upload-data";

    async function uploadRawData(rawData : string) {
        console.log(rawData);
        await fetch(SERVER_URL, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ data: rawData }),
        });
    }

    return (
        <SafeAreaView style={StyleSheet.absoluteFillObject}>
            <Stack.Screen options={{ headerShown: false }} />
            {Platform.OS === "android" ? <StatusBar hidden /> : null}

            {!scanned && (
                <CameraView
                    style={{
                        ...StyleSheet.absoluteFillObject,
                        top: 175,
                        bottom: 175,
                        left: 10,
                        right: 10,
                    }}
                    facing="back"
                    onBarcodeScanned={({ data }) => {
                        // locks camera while process the QR
                        setScanned(true);
                        uploadRawData(data);
                        Alert.alert(
                            "QR SCANNED",
                            "QR scanned successfully",
                            [
                                {
                                    text: "OK",
                                    onPress: () => {
                                        setScanned(false);
                                    },
                                },
                            ]
                        );
                    }}
                />
            )}
        </SafeAreaView>
    );
}