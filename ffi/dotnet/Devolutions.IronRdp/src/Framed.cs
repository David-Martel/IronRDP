using System;
using System.Buffers;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;

namespace Devolutions.IronRdp;

public class Framed<TS> where TS : Stream
{
    private const int ReadChunkSize = 8096;

    private readonly TS _stream;
    private List<byte> _buffer;
    private readonly SemaphoreSlim _writeLock = new(1, 1);

    public Framed(TS stream)
    {
        _stream = stream;
        _buffer = new List<byte>();
    }

    public (TS, List<byte>) GetInner()
    {
        return (_stream, _buffer);
    }

    public async Task<(Action, byte[])> ReadPdu()
    {
        var pdu = IronRdpPdu.New();
        while (true)
        {
            var pduInfo = pdu.FindSize(SnapshotBuffer());

            // Don't remove, FindSize is generated and can return null
            if (null != pduInfo)
            {
                var frame = await this.ReadExact(pduInfo.GetLength());
                var action = pduInfo.GetAction();
                return (action, frame);
            }
            else
            {
                var len = await this.Read();
                if (len == 0)
                {
                    throw new IronRdpLibException(IronRdpLibExceptionType.EndOfFile, "EOF on ReadPdu");
                }
            }
        }
    }

    /// <summary>
    /// Returns a span that represents a portion of the underlying buffer without modifying it.
    /// </summary>
    /// <remarks>Memory safety: the Framed instance should not be modified (any read operations) while span is in use.</remarks>
    /// <returns>A span that represents a portion of the underlying buffer.</returns>
    public Span<byte> Peek()
    {
        return CollectionsMarshal.AsSpan(this._buffer);
    }

    /// <summary>
    /// Reads from 0 to size bytes from the front of the buffer, and remove them from the buffer keeping the rest.
    /// </summary>
    /// <param name="size">The number of bytes to read.</param>
    /// <returns>An array of bytes containing the read data.</returns>
    public async Task<byte[]> ReadExact(nuint size)
    {
        var exactSize = checked((int)size);

        while (true)
        {
            if (_buffer.Count >= exactSize)
            {
                var result = new byte[exactSize];
                CollectionsMarshal.AsSpan(this._buffer)[..exactSize].CopyTo(result);
                this._buffer.RemoveRange(0, exactSize);
                return result;
            }

            var len = await this.Read();
            if (len == 0)
            {
                throw new Exception("EOF");
            }
        }
    }

    async Task<int> Read()
    {
        var rented = ArrayPool<byte>.Shared.Rent(ReadChunkSize);
        try
        {
            var size = await this._stream.ReadAsync(rented.AsMemory(0, ReadChunkSize));
            if (size > 0)
            {
                this._buffer.Capacity = Math.Max(this._buffer.Capacity, this._buffer.Count + size);
                for (var i = 0; i < size; i++)
                {
                    this._buffer.Add(rented[i]);
                }
            }

            return size;
        }
        finally
        {
            ArrayPool<byte>.Shared.Return(rented);
        }
    }

    public async Task Write(byte[] data)
    {
        await _writeLock.WaitAsync();
        try
        {
            ReadOnlyMemory<byte> memory = data;
            await _stream.WriteAsync(memory);
        }
        finally
        {
            _writeLock.Release();
        }
    }

    public async Task Write(WriteBuf buf)
    {
        var vecU8 = buf.GetFilled();
        var size = vecU8.GetSize();
        var bytesArray = new byte[size];
        vecU8.Fill(bytesArray);
        await Write(bytesArray);
    }


    /// <summary>
    /// Reads data from the buffer based on the provided PduHint.
    /// </summary>
    /// <param name="pduHint">The PduHint object used to determine the size of the data to read.</param>
    /// <returns>An asynchronous task that represents the operation. The task result contains the read data as a byte array.</returns>
    public async Task<byte[]> ReadByHint(PduHint pduHint)
    {
        while (true)
        {
            var size = pduHint.FindSize(SnapshotBuffer());
            if (size.IsSome())
            {
                return await this.ReadExact(size.Get());
            }
            else
            {
                var len = await this.Read();
                if (len == 0)
                {
                    throw new Exception("EOF");
                }
            }
        }
    }

    /// <summary>
    /// Reads data from the buffer based on a custom PDU hint function.
    /// </summary>
    /// <param name="customHint">A custom hint object implementing IPduHint interface.</param>
    /// <returns>An asynchronous task that represents the operation. The task result contains the read data as a byte array.</returns>
    public async Task<byte[]> ReadByHint(IPduHint customHint)
    {
        while (true)
        {
            var result = customHint.FindSize(SnapshotBuffer());
            if (result.HasValue)
            {
                return await this.ReadExact((nuint)result.Value.Item2);
            }
            else
            {
                var len = await this.Read();
                if (len == 0)
                {
                    throw new Exception("EOF");
                }
            }
        }
    }

    byte[] SnapshotBuffer()
    {
        return CollectionsMarshal.AsSpan(this._buffer).ToArray();
    }
}

/// <summary>
/// Interface for custom PDU hint implementations.
/// </summary>
public interface IPduHint
{
    /// <summary>
    /// Finds the size of a PDU in the given byte array.
    /// </summary>
    /// <param name="bytes">The byte array to analyze.</param>
    /// <returns>
    /// A tuple (detected, size) if PDU is detected, null if more bytes are needed.
    /// Throws exception if invalid PDU is detected.
    /// </returns>
    (bool, int)? FindSize(byte[] bytes);
}
