const server = Bun.serve({
  port: 3000,
  async fetch(req: Request) {
    const timestamp = new Date().toISOString();
    const method = req.method;
    const url = req.url;
    
    // Build headers object manually
    const headers: Record<string, string> = {};
    req.headers.forEach((value, key) => {
      headers[key] = value;
    });
    
    // Get body if present
    let body = null;
    try {
      if (req.body && (method === 'POST' || method === 'PUT' || method === 'PATCH')) {
        const clonedReq = req.clone();
        const contentType = headers['content-type'] || '';
        
        if (contentType.includes('application/json')) {
          body = await clonedReq.json();
        } else if (contentType.includes('application/x-www-form-urlencoded')) {
          body = await clonedReq.formData();
        } else {
          body = await clonedReq.text();
        }
      }
    } catch (e) {
      body = `[Error reading body: ${e}]`;
    }

    // Log the request
    console.log('\n' + '='.repeat(80));
    console.log(`📅 ${timestamp}`);
    console.log(`🔗 ${method} ${url}`);
    console.log('📋 Headers:', JSON.stringify(headers, null, 2));
    if (body !== null) {
      console.log('📦 Body:', typeof body === 'string' ? body : JSON.stringify(body, null, 2));
    }
    console.log('='.repeat(80));

    // Simple response
    return new Response(JSON.stringify({
      message: "Request logged successfully",
      timestamp,
      method,
      url: new URL(url).pathname,
      received: true
    }), {
      headers: {
        'Content-Type': 'application/json',
        'Access-Control-Allow-Origin': '*',
        'Access-Control-Allow-Methods': 'GET, POST, PUT, DELETE, PATCH, OPTIONS',
        'Access-Control-Allow-Headers': '*',
      },
      status: 200
    });
  },
});

console.log(`🚀 Server running on http://localhost:${server.port}`);
console.log('📝 All incoming requests will be logged with full details');
console.log('🔍 Watching for requests...\n');