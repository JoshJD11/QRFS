import { CameraView } from "expo-camera";
import {Link, Stack} from "expo-router";
import { useState } from "react";
import {
    Platform,
    SafeAreaView,
    StatusBar,
    StyleSheet,
    Alert,
    Button,
    View
} from "react-native";

export default function Home() {
    const [scanned, setScanned] = useState(false);
    const SERVER_DATA_URL = "http://192.168.0.3:3000/upload-data";
    const SERVER_FINISH_URL = "http://192.168.0.3:3000/finish-scanning";

    async function uploadRawData(rawData : string) {
        await fetch(SERVER_DATA_URL, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ data: rawData }),
        });
    }

    async function sendFinishSignal() {
        await fetch(SERVER_FINISH_URL, {
            method: "POST"
        });
        Alert.alert("Scan finished", "QR File System scanned successfully");
    }

    return (
        <SafeAreaView style={StyleSheet.absoluteFillObject}>
            <Stack.Screen options={{ headerShown: false }} />
            {Platform.OS === "android" ? <StatusBar hidden /> : null}

            <View style={{ marginTop: 700 }}>
                <Link href={"../(tabs)/scan"} asChild>
                    <Button title="End Scan" onPress={sendFinishSignal} />
                </Link>
            </View>

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
