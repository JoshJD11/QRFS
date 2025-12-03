import { Image } from 'expo-image';
import { View, StyleSheet } from 'react-native';

import { AnimatedCamera } from '@/components/animated-camera';
import ParallaxScrollView from '@/components/parallax-scroll-view';
import { ThemedText } from '@/components/themed-text';
import { ThemedView } from '@/components/themed-view';

export default function HomeScreen() {
  return (
    <ParallaxScrollView
      headerBackgroundColor={{ light: '#002855e6', dark: '#002855ff' }}
      headerImage={
        <View style={{ alignItems: 'center' }}><Image
          source={{ uri: 'https://www.tec.ac.cr/themes/custom/tecnologico/logo.svg'}} // rometo official TEC logo
          style={{
            width: 300,
            height: 275,
          }}
        /></View>
      }>
      <ThemedView style={styles.titleContainer}>
        <ThemedText type="title">QRFS Code Scanner</ThemedText>
        <AnimatedCamera />
      </ThemedView>
      <ThemedView style={styles.stepContainer}>
        <ThemedText type="subtitle">Step 1: Prepare the codes</ThemedText>
        <ThemedText>
          Have the{' '}
          <ThemedText type="defaultSemiBold">
            QR codes
          </ThemedText>
          {' '}ready and accessible before starting the scanning process.
        </ThemedText>
      </ThemedView>
      <ThemedView style={styles.stepContainer}>
        <ThemedText type="subtitle">Step 2: Grant permission</ThemedText>
        <ThemedText>
          Navigate to the{' '}
          <ThemedText type="defaultSemiBold">
            scanning page
          </ThemedText>
          {' '}and allow the app to access your device’s camera.
        </ThemedText>
      </ThemedView>
      <ThemedView style={styles.stepContainer}>
        <ThemedText type="subtitle">Step 3: Scan</ThemedText>
        <ThemedText>
          Tap the{' '}
          <ThemedText type="defaultSemiBold">
            Scan Code
          </ThemedText>
          {' '}button to begin scanning the QR codes.
        </ThemedText>
      </ThemedView>
      <View style={{ paddingVertical: 16, alignItems: 'center' }}>
        <ThemedText type="default" style={{ textAlign: 'center', opacity: 0.5 }}>
          App made by:{"\n"}JoshJD11 - K-lobiTo - Sebco27
        </ThemedText>
      </View>
    </ParallaxScrollView>
    
  );
}

const styles = StyleSheet.create({
  titleContainer: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
  },
  stepContainer: {
    gap: 8,
    marginBottom: 8,
  }
});
