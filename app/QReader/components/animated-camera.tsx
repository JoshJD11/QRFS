import Animated from 'react-native-reanimated';

export function AnimatedCamera() {
  return (
    <Animated.Text
      style={{
        fontSize: 28,
        lineHeight: 32,
        marginTop: 0,
        marginLeft: 5,
        animationName: {
          '50%': { transform: [{ rotate: '25deg' }] },
        },
        animationIterationCount: 4,
        animationDuration: '300ms',
      }}>
      📷
    </Animated.Text>
  );
}
