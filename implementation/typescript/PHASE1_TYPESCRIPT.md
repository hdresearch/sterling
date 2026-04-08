# Phase 1: Complete TypeScript SDK Generation

## Current State Analysis
- Basic client structure exists in `templates/typescript/`
- Missing: complete operation generation, package.json, TypeScript interfaces
- Target: Transform 40% → 100% completion

## Week 1: Core TypeScript Infrastructure

### 1.1 Enhanced TypeScript Client Template
```typescript
// templates/typescript/client.ts.template
import axios, { AxiosInstance, AxiosRequestConfig, AxiosResponse } from 'axios';

export interface ClientConfig {
  baseURL: string;
  apiKey?: string;
  bearerToken?: string;
  timeout?: number;
  retries?: number;
}

export class {{clientName}} {
  private http: AxiosInstance;
  private config: ClientConfig;

  constructor(config: ClientConfig) {
    this.config = config;
    this.http = axios.create({
      baseURL: config.baseURL,
      timeout: config.timeout || 30000,
      headers: this.buildHeaders(),
    });
    
    this.setupInterceptors();
  }

  private buildHeaders(): Record<string, string> {
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
      'User-Agent': '{{clientName}}/{{version}}',
    };

    if (this.config.apiKey) {
      headers['X-API-Key'] = this.config.apiKey;
    }
    
    if (this.config.bearerToken) {
      headers['Authorization'] = `Bearer ${this.config.bearerToken}`;
    }

    return headers;
  }

  {{#operations}}
  async {{operationName}}({{#parameters}}{{name}}: {{type}}{{#optional}}?{{/optional}}{{#hasMore}}, {{/hasMore}}{{/parameters}}): Promise<{{returnType}}> {
    const response = await this.http.{{httpMethod}}('{{path}}'{{#hasBody}}, {{bodyParam}}{{/hasBody}});
    return response.data;
  }
  {{/operations}}
}
```

### 1.2 TypeScript Interface Generation
```typescript
// templates/typescript/types.ts.template
{{#models}}
export interface {{name}} {
  {{#properties}}
  {{name}}{{#optional}}?{{/optional}}: {{type}};
  {{/properties}}
}
{{/models}}

{{#enums}}
export enum {{name}} {
  {{#values}}
  {{name}} = '{{value}}',
  {{/values}}
}
{{/enums}}
```

### 1.3 Package.json Generation
```json
{
  "name": "{{packageName}}",
  "version": "{{version}}",
  "description": "TypeScript SDK for {{apiName}}",
  "main": "dist/index.js",
  "module": "dist/index.esm.js",
  "types": "dist/index.d.ts",
  "exports": {
    ".": {
      "import": "./dist/index.esm.js",
      "require": "./dist/index.js",
      "types": "./dist/index.d.ts"
    }
  },
  "scripts": {
    "build": "rollup -c",
    "test": "jest",
    "lint": "eslint src/**/*.ts",
    "type-check": "tsc --noEmit"
  },
  "dependencies": {
    "axios": "^1.6.0"
  },
  "devDependencies": {
    "@types/node": "^20.0.0",
    "typescript": "^5.0.0",
    "rollup": "^4.0.0",
    "@rollup/plugin-typescript": "^11.0.0",
    "jest": "^29.0.0",
    "@types/jest": "^29.0.0",
    "eslint": "^8.0.0",
    "@typescript-eslint/eslint-plugin": "^6.0.0"
  }
}
```

## Week 2: Advanced TypeScript Features

### 2.1 Error Handling System
```typescript
// templates/typescript/errors.ts.template
export class {{clientName}}Error extends Error {
  constructor(
    message: string,
    public statusCode?: number,
    public response?: any
  ) {
    super(message);
    this.name = '{{clientName}}Error';
  }
}

export class ValidationError extends {{clientName}}Error {
  constructor(message: string, public errors: any[]) {
    super(message, 400);
    this.name = 'ValidationError';
  }
}

export class AuthenticationError extends {{clientName}}Error {
  constructor(message: string = 'Authentication failed') {
    super(message, 401);
    this.name = 'AuthenticationError';
  }
}
```

### 2.2 Request/Response Interceptors
```typescript
private setupInterceptors(): void {
  // Request interceptor
  this.http.interceptors.request.use(
    (config) => {
      // Add request ID for tracing
      config.headers['X-Request-ID'] = this.generateRequestId();
      return config;
    },
    (error) => Promise.reject(error)
  );

  // Response interceptor
  this.http.interceptors.response.use(
    (response) => response,
    (error) => {
      if (error.response?.status === 401) {
        throw new AuthenticationError();
      }
      if (error.response?.status === 400) {
        throw new ValidationError(
          error.response.data.message,
          error.response.data.errors
        );
      }
      throw new {{clientName}}Error(
        error.message,
        error.response?.status,
        error.response?.data
      );
    }
  );
}
```

## Week 3: TypeScript Polish & Testing

### 3.1 Build Configuration (rollup.config.js)
```javascript
import typescript from '@rollup/plugin-typescript';

export default [
  // ESM build
  {
    input: 'src/index.ts',
    output: {
      file: 'dist/index.esm.js',
      format: 'es',
    },
    plugins: [typescript()],
  },
  // CommonJS build
  {
    input: 'src/index.ts',
    output: {
      file: 'dist/index.js',
      format: 'cjs',
    },
    plugins: [typescript()],
  },
];
```

### 3.2 Test Generation
```typescript
// templates/typescript/tests.ts.template
import { {{clientName}} } from '../src';

describe('{{clientName}}', () => {
  let client: {{clientName}};

  beforeEach(() => {
    client = new {{clientName}}({
      baseURL: 'https://api.example.com',
      apiKey: 'test-key',
    });
  });

  {{#operations}}
  describe('{{operationName}}', () => {
    it('should make correct API call', async () => {
      // Mock implementation
      const mockResponse = {{mockResponse}};
      jest.spyOn(client['http'], '{{httpMethod}}').mockResolvedValue({
        data: mockResponse,
      });

      const result = await client.{{operationName}}({{testParams}});
      
      expect(result).toEqual(mockResponse);
      expect(client['http'].{{httpMethod}}).toHaveBeenCalledWith(
        '{{path}}'{{#hasBody}}, {{testBody}}{{/hasBody}}
      );
    });
  });
  {{/operations}}
});
```

## Implementation Tasks

### Zig Code Changes Required

1. **Enhance TypeScript Template Engine** (`src/generator/template.zig`)
```zig
// Add TypeScript-specific template processing
fn generateTypeScriptTypes(self: *TemplateEngine, schema: OpenAPISchema) ![]u8 {
    var types = std.ArrayList(u8).init(self.allocator);
    defer types.deinit();
    
    for (schema.components.schemas) |model| {
        try self.generateTypeScriptInterface(&types, model);
    }
    
    return types.toOwnedSlice();
}

fn generateTypeScriptInterface(self: *TemplateEngine, writer: *std.ArrayList(u8), model: SchemaModel) !void {
    try writer.appendSlice("export interface ");
    try writer.appendSlice(model.name);
    try writer.appendSlice(" {\n");
    
    for (model.properties) |prop| {
        try writer.appendSlice("  ");
        try writer.appendSlice(prop.name);
        if (prop.optional) try writer.appendSlice("?");
        try writer.appendSlice(": ");
        try writer.appendSlice(self.mapToTypeScriptType(prop.type));
        try writer.appendSlice(";\n");
    }
    
    try writer.appendSlice("}\n\n");
}
```

2. **Add Package.json Generation** (`src/generator/sdk.zig`)
```zig
fn generatePackageJson(self: *SDKGenerator, config: Config) !void {
    const template = try self.loadTemplate("package.json.template");
    const context = PackageJsonContext{
        .packageName = config.project.name,
        .version = config.project.version,
        .apiName = self.spec.info.title,
    };
    
    const content = try self.template_engine.render(template, context);
    try self.writeFile("package.json", content);
}
```

3. **Enhanced Operation Generation**
```zig
fn generateTypeScriptOperations(self: *SDKGenerator) !void {
    var operations = std.ArrayList(Operation).init(self.allocator);
    defer operations.deinit();
    
    for (self.spec.paths) |path| {
        for (path.operations) |op| {
            const ts_op = Operation{
                .name = try self.toTypeScriptMethodName(op.operationId),
                .httpMethod = try self.toTypeScriptHttpMethod(op.method),
                .path = path.path,
                .parameters = try self.convertParameters(op.parameters),
                .returnType = try self.mapResponseType(op.responses),
            };
            try operations.append(ts_op);
        }
    }
    
    const template = try self.loadTemplate("client.ts.template");
    const content = try self.template_engine.render(template, .{ .operations = operations.items });
    try self.writeFile("src/client.ts", content);
}
```

## Success Criteria

- [ ] Generated TypeScript code compiles without errors
- [ ] All HTTP methods (GET, POST, PUT, DELETE, PATCH) supported
- [ ] Type-safe request/response handling
- [ ] Comprehensive error handling
- [ ] ESM/CJS dual module support
- [ ] Complete test suite generation
- [ ] Documentation generation with JSDoc
- [ ] Package ready for npm publishing

## Testing Strategy

1. **Unit Tests**: Test each generated method
2. **Integration Tests**: Test against real API endpoints
3. **Type Tests**: Verify TypeScript type safety
4. **Build Tests**: Ensure ESM/CJS builds work
5. **Performance Tests**: Measure bundle size and runtime performance

This phase will elevate TypeScript SDK generation from 40% to 100% completion, making it production-ready and competitive with commercial solutions.
