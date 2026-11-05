// From the photo the camera hands back to the original the device commits
// to: the whole frame, scaled down to at most 2^19 pixels.

import { fit, type Channels, type Size } from "./fingerprint";

export interface Framing {
  canvas: HTMLCanvasElement;
  /// The original's size, which the device signs with the fingerprint.
  size: Size;
  /// The pixels as the fingerprint takes them, and the lossless PNG of the same bytes.
  original(): Promise<{ channels: Channels; png: Blob }>;
}

export async function framing(file: File, canvas: HTMLCanvasElement): Promise<Framing> {
  const photo = await load(file);
  const size = fit(photo.naturalWidth, photo.naturalHeight);
  canvas.width = size.width;
  canvas.height = size.height;
  // Opaque, so the PNG and the pixel buffer describe the same bytes.
  const context = canvas.getContext("2d", { alpha: false, willReadFrequently: true });
  if (!context) throw new Error("no canvas");
  context.imageSmoothingQuality = "high";
  context.drawImage(photo, 0, 0, size.width, size.height);

  const original = async () => {
    const rgba = context.getImageData(0, 0, size.width, size.height).data;
    const channels = [0, 1, 2].map((offset) => {
      const channel = new Uint8Array(size.width * size.height);
      for (let i = 0; i < channel.length; i++) channel[i] = rgba[i * 4 + offset]!;
      return channel;
    }) as Channels;
    const png = await new Promise<Blob>((resolve, reject) =>
      canvas.toBlob((blob) => (blob ? resolve(blob) : reject(new Error("PNG encoding failed"))), "image/png"),
    );
    return { channels, png };
  };
  return { canvas, size, original };
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
