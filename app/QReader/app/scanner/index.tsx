import { CameraView } from "expo-camera";
import { Stack } from "expo-router";
import { useState, useEffect } from "react";
import { Image } from 'expo-image';
import {
    Platform,
    SafeAreaView,
    StatusBar,
    StyleSheet,
    Alert,
} from "react-native";

type ChunkMap = { [id: number]: string };

export default function Home() {
    const [scanned, setScanned] = useState(false);
    const [indexScanned, setIndexScanned] = useState(false);
    const [imageBase64, setImageBase64] = useState<string | null>(null);
    const [mime, setMime] = useState<string | null>(null);
    const [base64len, setBase64len] = useState<number | null>(null);
    const [chunks, setChunks] = useState<ChunkMap>({});

    // join QR data
    useEffect(() => {
        if (base64len === null) return;

        const ids = Object.keys(chunks).map(Number);
        if (ids.length !== base64len) return;

        ids.sort((a, b) => a - b);
        const dataCombined = ids.map((id) => chunks[id]).join("");

        setImageBase64(dataCombined);

        setScanned(true); // stop scanning
    }, [chunks, base64len]);

    return (
        <SafeAreaView style={StyleSheet.absoluteFillObject}>
            <Stack.Screen options={{ headerShown: false }} />
            {Platform.OS === "android" ? <StatusBar hidden /> : null}
            {imageBase64 && mime && (
                <Image source={{ uri: `data:${mime};base64,${imageBase64}`}}
                       style={{
                           width: 300,
                           height: 275,
                           resizeMode: "contain",
                       }} />
            )}
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

                        setScanned(true); // locks camera while process the QR

                        if (!indexScanned) {
                            try {
                                const json = JSON.parse(data);

                                if (json.type === "image-base64") {
                                    setMime(json.mime);
                                    setBase64len(json.len);
                                    setIndexScanned(true);

                                    Alert.alert(
                                        "QR SCANNED",
                                        "Index QR scanned successfully",
                                        [
                                            {
                                                text: "OK",
                                                onPress: () => {
                                                    setScanned(false);
                                                },
                                            },
                                        ]
                                    );
                                } else {
                                    setScanned(false);
                                }
                            } catch (e) {
                                console.warn("QR inválido:", e);
                                setScanned(false);
                            }
                        } else { // now we scan the QR with data
                            try {
                                const json = JSON.parse(data);

                                const isValid =
                                    base64len !== null &&
                                    json.id > 0 &&
                                    json.id <= base64len &&
                                    !(json.id in chunks);

                                if (isValid) {
                                    setChunks((prev) => ({
                                        ...prev,
                                        [json.id]: json.data,
                                    }));

                                    Alert.alert(
                                        "QR SCANNED",
                                        `QR ${json.id} scanned successfully`,
                                        [ { text: "OK" } ]
                                    );
                                }
                            } catch (e) {
                                console.warn("QR inválido:", e);
                            } finally {
                                setScanned(false);
                            }
                        }
                    }}
                />
            )}
        </SafeAreaView>
    );
}