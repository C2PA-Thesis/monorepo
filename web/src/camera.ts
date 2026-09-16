// From the photo the camera hands back to the 1024x512 original the device
// commits to: a 2:1 window the photographer positions, then a resize.

import { HEIGHT, WIDTH, type Channels } from "./fingerprint";

export interface Framing {
  canvas: HTMLCanvasElement;
  /// Redraws with the window at `position`, 0 to 1 along the photo's longer axis.
  frame(position: number): void;
  /// The pixels as the fingerprint takes them, and the lossless PNG of the same bytes.
  original(): Promise<{ channels: Channels; png: Blob }>;
}

export async function framing(file: File, canvas: HTMLCanvasElement): Promise<Framing> {
  const photo = await load(file);
  // Opaque, so the PNG and the pixel buffer describe the same bytes.
  const context = canvas.getContext("2d", { alpha: false, willReadFrequently: true });
  if (!context) throw new Error("no canvas");

  const wide = photo.width / photo.height >= WIDTH / HEIGHT;
  const window = wide
    ? { width: (photo.height * WIDTH) / HEIGHT, height: photo.height }
    : { width: photo.width, height: (photo.width * HEIGHT) / WIDTH };
  const frame = (position: number) => {
    const x = wide ? (photo.width - window.width) * position : 0;
    const y = wide ? 0 : (photo.height - window.height) * position;
    context.drawImage(photo, x, y, window.width, window.height, 0, 0, WIDTH, HEIGHT);
  };
  frame(0.5);

  const original = async () => {
    const rgba = context.getImageData(0, 0, WIDTH, HEIGHT).data;
    const channels = [0, 1, 2].map((offset) => {
      const channel = new Uint8Array(WIDTH * HEIGHT);
      for (let i = 0; i < channel.length; i++) channel[i] = rgba[i * 4 + offset]!;
      return channel;
    }) as Channels;
    const png = await new Promise<Blob>((resolve, reject) =>
      canvas.toBlob((blob) => (blob ? resolve(blob) : reject(new Error("PNG encoding failed"))), "image/png"),
    );
    return { channels, png };
  };
  return { canvas, frame, original };
}

/// An <img> honors the EXIF orientation, so a portrait photo comes in upright.
function load(file: File): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => {
      URL.revokeObjectURL(image.src);
      resolve(image);
    };
    image.onerror = () => reject(new Error("the photo could not be decoded"));
    image.src = URL.createObjectURL(file);
  });
}

export async function base64(blob: Blob): Promise<string> {
  const bytes = new Uint8Array(await blob.arrayBuffer());
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(binary);
}
